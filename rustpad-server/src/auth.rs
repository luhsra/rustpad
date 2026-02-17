use anyhow::{Context, Result};
use dashmap::DashMap;
use log::{error, info};
use openidconnect::core::{
    CoreAuthenticationFlow, CoreClient, CoreGenderClaim, CoreIdTokenClaims, CoreIdTokenVerifier,
    CoreProviderMetadata,
};
use openidconnect::{AccessTokenHash, AdditionalClaims, UserInfoClaims};
use openidconnect::{
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, IssuerUrl, Nonce, OAuth2TokenResponse,
    RedirectUrl, Scope,
};
use openidconnect::{EndpointMaybeSet, EndpointNotSet, EndpointSet, reqwest};
use serde::{Deserialize, Serialize};
use warp::reject::Rejection;
use warp::reply::Reply;

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::ServerState;
use crate::rustpad::UserInfo;
use crate::util::{Identifier, Session};

/// Time after which a login attempt expires if not completed.
const LOGINGIN_EXPIRE_SEC: u64 = 15 * 60;
/// Time after which a logged in session expires.
pub const LOGGEDIN_EXPIRE_SEC: u64 = 2 * 24 * 60 * 60;

#[derive(Debug)]
struct AuthError;
impl warp::reject::Reject for AuthError {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub name: String,
    pub admin: bool,
    pub hue: u16,
}
impl From<User> for UserInfo {
    fn from(user: User) -> Self {
        Self {
            name: user.name,
            hue: user.hue,
            admin: user.admin,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct OpenIdConfig {
    client_id: String,
    client_secret: String,
    issuer_url: String,
    host_url: String,
    admin_group: String,
}

#[derive(Debug, Clone)]
enum AuthState {
    LoggingIn {
        csrf_token: CsrfToken,
        nonce: Nonce,
        expires_at: Instant,
        redirect: Option<Identifier>,
    },
    LoggedIn {
        user: User,
        expires_at: Instant,
    },
}

#[derive(Debug)]
pub struct UserSessions {
    openid: Option<OpenIdState>,
    sessions: DashMap<Session, AuthState>,
}

#[derive(Debug)]
pub struct OpenIdState {
    client: CoreClient<
        EndpointSet,      // AuthUrl
        EndpointNotSet,   // DeviceAuthUrl
        EndpointNotSet,   // IntrospectionUrl
        EndpointNotSet,   // RevocationUrl
        EndpointMaybeSet, // TokenUrl
        EndpointMaybeSet, // UserInfoUrl
    >,
    http_client: reqwest::Client,
    admin_group: String,
}
impl OpenIdState {
    pub async fn new(config: OpenIdConfig) -> Result<Self> {
        let issuer_url = IssuerUrl::new(config.issuer_url).context("Invalid issuer URL")?;

        let http_client = reqwest::ClientBuilder::new()
            // Following redirects opens the client up to SSRF vulnerabilities.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .context("Failed to build HTTP client")?;

        // Fetch OpenID Connect discovery document.
        let provider_metadata = CoreProviderMetadata::discover_async(issuer_url, &http_client)
            .await
            .context("Failed to discover OpenID Provider")?;

        let redirect_url = RedirectUrl::new(config.host_url + "/api/authorized")
            .context("Invalid redirect URL")?;

        // Set up the config for the GitLab OAuth2 process.
        let client = CoreClient::from_provider_metadata(
            provider_metadata,
            ClientId::new(config.client_id),
            Some(ClientSecret::new(config.client_secret)),
        )
        .set_redirect_uri(redirect_url);

        Ok(Self {
            client,
            http_client,
            admin_group: config.admin_group,
        })
    }
}

impl UserSessions {
    pub fn new(openid: Option<OpenIdState>) -> Self {
        Self {
            openid,
            sessions: DashMap::new(),
        }
    }

    pub async fn get_user(&self, session: &Session) -> Option<User> {
        let login_state = self.sessions.get(session)?;
        let AuthState::LoggedIn { user, expires_at } = &*login_state else {
            return None;
        };
        if *expires_at < Instant::now() {
            self.sessions.remove(session);
            return None;
        }
        Some(user.clone())
    }
}

#[derive(Debug, Deserialize)]
pub struct LoginQuery {
    pub redirect: Option<Identifier>,
}

pub async fn login(
    auth: Arc<ServerState>,
    session: Session,
    query: LoginQuery,
) -> Result<impl Reply, Rejection> {
    let auth = &auth.users;
    let Some(openid) = &auth.openid else {
        return Err(warp::reject::custom(AuthError));
    };
    // Generate the full authorization URL.
    let (auth_url, csrf_token, nonce) = openid
        .client
        .authorize_url(
            CoreAuthenticationFlow::AuthorizationCode,
            CsrfToken::new_random,
            Nonce::new_random,
        )
        // Set the desired scopes.
        .add_scope(Scope::new("openid".to_string()))
        .add_scope(Scope::new("profile".to_string()))
        .add_scope(Scope::new("email".to_string()))
        .url();

    // Store the CSRF token and nonce in the logins map with an expiration time.
    let expires_at = Instant::now() + Duration::from_secs(LOGINGIN_EXPIRE_SEC);
    auth.sessions.retain(|_, state| match state {
        AuthState::LoggingIn { expires_at, .. } => *expires_at > Instant::now(),
        AuthState::LoggedIn { expires_at, .. } => *expires_at > Instant::now(),
    });

    info!("Starting login for session {session}, redirecting to {auth_url}",);
    auth.sessions.insert(
        session,
        AuthState::LoggingIn {
            csrf_token,
            nonce,
            expires_at,
            redirect: query.redirect,
        },
    );

    // Redirect the user to the authorization URL.
    Ok(warp::redirect(
        auth_url.as_str().parse::<warp::http::Uri>().unwrap(),
    ))
}

#[derive(Debug, Deserialize)]
pub struct AuthorizedQuery {
    pub code: AuthorizationCode,
    pub state: CsrfToken,
}

pub async fn authorized(
    state: Arc<ServerState>,
    session: Session,
    query: AuthorizedQuery,
) -> Result<impl Reply, Rejection> {
    let auth = &state.users;
    let err = |err: Option<&dyn std::error::Error>, message: &str| {
        error!("{message}: {err:?}");
        auth.sessions.remove(&session);
        warp::reject::custom(AuthError)
    };

    let AuthorizedQuery { code, state } = query;
    info!(
        "Received authorization code: {code:?}, state: {state:?} for session {:?}",
        session
    );
    let Some(openid) = &auth.openid else {
        return Err(err(None, "OpenID Connect is not configured"));
    };

    let mut login_state = auth
        .sessions
        .get_mut(&session)
        .ok_or_else(|| err(None, "No login state found for session"))?;

    let AuthState::LoggingIn {
        csrf_token,
        nonce,
        expires_at,
        redirect,
    } = &*login_state
    else {
        return Err(warp::reject::custom(AuthError));
    };

    if *expires_at < Instant::now() {
        return Err(err(None, "Login attempt expired"));
    }

    // Timing attack safe comparison (expensive but safe)
    if csrf_token != &state {
        return Err(err(None, "Invalid CSRF token"));
    }

    // Now you can exchange it for an access token and ID token.
    let token_response = openid
        .client
        .exchange_code(code)
        .map_err(|e| err(Some(&e), "Failed to exchange code for token"))?
        .request_async(&openid.http_client)
        .await
        .map_err(|e| err(Some(&e), "Failed to contact token endpoint"))?;

    // Extract the claims from the token response.
    let id_token_verifier: CoreIdTokenVerifier = openid.client.id_token_verifier();

    let id_token = token_response
        .extra_fields()
        .id_token()
        .ok_or_else(|| err(None, "Server did not return an ID token"))?;

    let claims: &CoreIdTokenClaims = id_token
        .claims(&id_token_verifier, nonce)
        .map_err(|e| err(Some(&e), "Failed to verify ID token"))?;
    // info!("ID token claims: {claims:?}");

    // Verify the access token hash to ensure that the access token hasn't been substituted for
    // another user's.
    if let Some(expected_token_hash) = claims.access_token_hash() {
        let actual_token_hash = AccessTokenHash::from_token(
            token_response.access_token(),
            id_token
                .signing_alg()
                .map_err(|e| err(Some(&e), "ID token is missing signing algorithm"))?,
            id_token
                .signing_key(&id_token_verifier)
                .map_err(|e| err(Some(&e), "Failed signing key for ID token"))?,
        )
        .map_err(|e| err(Some(&e), "Failed to compute access token hash"))?;
        if actual_token_hash != *expected_token_hash {
            return Err(err(None, "Invalid access token"));
        }
    }

    // Request the user info from the user info endpoint.
    let userinfo_claims: UserInfoClaims<GitLabClaims, CoreGenderClaim> = openid
        .client
        .user_info(token_response.access_token().to_owned(), None)
        .map_err(|e| err(Some(&e), "No user info endpoint"))?
        .request_async(&openid.http_client)
        .await
        .map_err(|e| err(Some(&e), "Failed to request user info"))?;
    info!("User info claims: {userinfo_claims:?}");

    // Create a new user session.
    let user = User {
        name: claims
            .preferred_username()
            .map(|s| s.to_string())
            .ok_or_else(|| err(None, "ID token is missing name claim"))?,
        admin: userinfo_claims
            .additional_claims()
            .groups
            .contains(&openid.admin_group),
        hue: rand::random_range(0..360),
    };
    info!("Authenticated user: {user:?}");

    // Store the user session in the sessions map.
    let redirect_url = if let Some(redirect) = redirect {
        format!("/#{redirect}")
    } else {
        "/".to_string()
    };

    *login_state = AuthState::LoggedIn {
        user,
        expires_at: Instant::now() + Duration::from_secs(LOGGEDIN_EXPIRE_SEC),
    };

    info!("User logged in successfully for session, redirecting to {redirect_url}");

    Ok(warp::reply::html(format!(
        r#"
        <html>
            <head>
                <meta http-equiv="refresh" content="0; URL={redirect_url}" />
            </head>
            <body>
                <p>Login successful! Redirecting...</p>
                <p>Or <a href="{redirect_url}">click here</a>.</p>
            </body>
        </html>
        "#
    )))
}

pub async fn logout(state: Arc<ServerState>, session: Session) -> Result<impl Reply, Rejection> {
    state.users.sessions.remove(&session);
    state.users.sessions.retain(|_, state| match state {
        AuthState::LoggingIn { expires_at, .. } => *expires_at > Instant::now(),
        AuthState::LoggedIn { expires_at, .. } => *expires_at > Instant::now(),
    });
    Ok(warp::redirect("/".parse::<warp::http::Uri>().unwrap()))
}

#[derive(Debug, Deserialize, Serialize)]
struct GitLabClaims {
    groups: Vec<String>,
}

impl AdditionalClaims for GitLabClaims {}
