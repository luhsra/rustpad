use anyhow::{Context, Result, anyhow};
use axum::Router;
use axum::extract::{Query, State};
use axum::response::{Html, IntoResponse, Redirect};
use axum::routing::get;
use dashmap::DashMap;
use openidconnect::core::{
    CoreAuthDisplay, CoreAuthPrompt, CoreAuthenticationFlow, CoreClaimName, CoreClaimType,
    CoreClientAuthMethod, CoreErrorResponseType, CoreGenderClaim, CoreGrantType,
    CoreIdTokenVerifier, CoreJsonWebKey, CoreJweContentEncryptionAlgorithm,
    CoreJweKeyManagementAlgorithm, CoreJwsSigningAlgorithm, CoreResponseMode, CoreResponseType,
    CoreRevocableToken, CoreRevocationErrorResponse, CoreSubjectIdentifierType,
    CoreTokenIntrospectionResponse, CoreTokenType,
};
use openidconnect::{
    AccessTokenHash, AdditionalClaims, AdditionalProviderMetadata, AuthorizationCode, Client,
    ClientId, ClientSecret, CsrfToken, EmptyExtraTokenFields, EndpointMaybeSet, EndpointNotSet,
    EndpointSet, IdTokenClaims, IdTokenFields, IssuerUrl, Nonce, OAuth2TokenResponse,
    PkceCodeChallenge, PkceCodeVerifier, ProviderMetadata, RedirectUrl, RevocationUrl, Scope,
    StandardErrorResponse, StandardTokenResponse, reqwest,
};
use serde::{Deserialize, Serialize};
use tracing::{error, info};

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::util::{AppError, Identifier, Session};

/// Time after which a login attempt expires if not completed.
const LOGINGIN_EXPIRE_SEC: u64 = 15 * 60;
/// Time after which a logged in session expires.
pub const LOGGEDIN_EXPIRE_SEC: u64 = 2 * 24 * 60 * 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub name: String,
    pub hue: u16,
    #[serde(default)]
    pub admin: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct OpenIdConfig {
    client_id: String,
    client_secret: String,
    issuer_url: String,
    host_url: String,
    admin_group: String,
}

#[derive(Debug)]
enum AuthState {
    LoggingIn {
        csrf_token: CsrfToken,
        pkce_verifier: PkceCodeVerifier,
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
    sessions: DashMap<Session, AuthState>,
    client: Client<
        GitLabTokenClaims,
        CoreAuthDisplay,
        CoreGenderClaim,
        CoreJweContentEncryptionAlgorithm,
        CoreJsonWebKey,
        CoreAuthPrompt,
        StandardErrorResponse<CoreErrorResponseType>,
        StandardTokenResponse<GitLabIdTokenFields, CoreTokenType>,
        CoreTokenIntrospectionResponse,
        CoreRevocableToken,
        CoreRevocationErrorResponse,
        EndpointSet,      // HasAuthUrl,
        EndpointNotSet,   // HasDeviceAuthUrl,
        EndpointNotSet,   // HasIntrospectionUrl,
        EndpointSet,      // HasRevocationUrl,
        EndpointMaybeSet, // HasTokenUrl,
        EndpointMaybeSet, // HasUserInfoUrl,
    >,
    http_client: reqwest::Client,
    admin_group: String,
}

impl UserSessions {
    pub async fn new(config: OpenIdConfig) -> Result<Self> {
        let issuer_url = IssuerUrl::new(config.issuer_url).context("Invalid issuer URL")?;

        let http_client = reqwest::ClientBuilder::new()
            // Following redirects opens the client up to SSRF vulnerabilities.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .context("Failed to build HTTP client")?;

        // Fetch OpenID Connect discovery document.
        let provider_metadata =
            ProviderMetadataWithRevocation::discover_async(issuer_url, &http_client)
                .await
                .context("Failed to discover OpenID Provider")?;

        let redirect_url = RedirectUrl::new(config.host_url + "/auth/authorized")
            .context("Invalid redirect URL")?;

        // Set up the config for the GitLab OAuth2 process.
        let revocation_url = provider_metadata
            .additional_metadata()
            .revocation_endpoint
            .clone();
        let client = Client::from_provider_metadata(
            provider_metadata,
            ClientId::new(config.client_id),
            Some(ClientSecret::new(config.client_secret)),
        )
        .set_redirect_uri(redirect_url)
        .set_revocation_url(revocation_url);

        Ok(Self {
            client,
            http_client,
            admin_group: config.admin_group,
            sessions: DashMap::new(),
        })
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

    pub async fn update_user(&self, session: &Session, user: User) {
        if let Some(mut login_state) = self.sessions.get_mut(session) {
            let AuthState::LoggedIn {
                user: existing_user,
                expires_at,
            } = &mut *login_state
            else {
                return;
            };
            if *expires_at < Instant::now() {
                self.sessions.remove(session);
                return;
            }
            *existing_user = user;
        }
    }
}

pub fn routes(users: Option<Arc<UserSessions>>) -> Router {
    if let Some(users) = users {
        Router::new()
            .route("/login", get(login))
            .route("/authorized", get(authorized))
            .route("/logout", get(logout))
            .with_state(users)
    } else {
        Router::new()
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct RedirectQuery {
    pub redirect: Option<Identifier>,
}

pub async fn login(
    State(users): State<Arc<UserSessions>>,
    Query(query): Query<RedirectQuery>,
) -> Result<impl IntoResponse, AppError> {
    let session = Session::new();

    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

    // Generate the full authorization URL.
    let (auth_url, csrf_token, nonce) = users
        .client
        .authorize_url(
            CoreAuthenticationFlow::AuthorizationCode,
            CsrfToken::new_random,
            Nonce::new_random,
        )
        // Set the desired scopes.
        .add_scope(Scope::new("openid".to_string()))
        // .add_scope(Scope::new("profile".to_string()))
        // .add_scope(Scope::new("email".to_string()))
        .set_pkce_challenge(pkce_challenge)
        .url();

    // Store the CSRF token and nonce in the logins map with an expiration time.
    let expires_at = Instant::now() + Duration::from_secs(LOGINGIN_EXPIRE_SEC);
    users.sessions.retain(|_, state| match state {
        AuthState::LoggingIn { expires_at, .. } => *expires_at > Instant::now(),
        AuthState::LoggedIn { expires_at, .. } => *expires_at > Instant::now(),
    });

    info!(
        "Login {session}: -> {}",
        auth_url.domain().unwrap_or_default()
    );
    users.sessions.insert(
        session.clone(),
        AuthState::LoggingIn {
            csrf_token,
            pkce_verifier,
            nonce,
            expires_at,
            redirect: query.redirect,
        },
    );

    // Redirect the user to the authorization URL.
    Ok(session
        .set_cookie(Redirect::to(auth_url.as_str()))
        .into_response())
}

#[derive(Debug, Deserialize)]
pub struct AuthorizedQuery {
    pub code: AuthorizationCode,
    pub state: CsrfToken,
}

pub async fn authorized(
    State(users): State<Arc<UserSessions>>,
    session: Session,
    Query(query): Query<AuthorizedQuery>,
) -> Result<impl IntoResponse, AppError> {
    let err = |err: Option<&dyn std::error::Error>, message: &str| {
        error!("{message}: {err:?}");
        AppError(anyhow!("{message}: {err:?}"))
    };

    let AuthorizedQuery { code, state } = query;
    info!("Authorize {session}");

    let (_, login_state) = users
        .sessions
        .remove(&session)
        .ok_or_else(|| err(None, "No login state found for session"))?;

    let AuthState::LoggingIn {
        csrf_token,
        pkce_verifier,
        nonce,
        expires_at,
        redirect,
    } = login_state
    else {
        return Err(err(None, "Session is not in logging in state"));
    };

    if expires_at < Instant::now() {
        return Err(err(None, "Login attempt expired"));
    }

    // Timing attack safe comparison (expensive but safe)
    if csrf_token != state {
        return Err(err(None, "Invalid CSRF token"));
    }

    // Now you can exchange it for an access token and ID token.
    let token_response = users
        .client
        .exchange_code(code)
        .map_err(|e| err(Some(&e), "Failed to exchange code for token"))?
        .set_pkce_verifier(pkce_verifier)
        .request_async(&users.http_client)
        .await
        .map_err(|e| err(Some(&e), "Failed to contact token endpoint"))?;

    // Extract the claims from the token response.
    let id_token_verifier: CoreIdTokenVerifier = users.client.id_token_verifier();

    let id_token = token_response
        .extra_fields()
        .id_token()
        .ok_or_else(|| err(None, "Server did not return an ID token"))?;

    let claims: &GitLabIdTokenClaims = id_token
        .claims(&id_token_verifier, &nonce)
        .map_err(|e| err(Some(&e), "Failed to verify ID token"))?;
    info!("ID token claims: {claims:?}");

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

    // Create a new user session.
    let user = User {
        name: claims
            .preferred_username()
            .map(|s| s.to_string())
            .ok_or_else(|| err(None, "ID token is missing name claim"))?,
        admin: claims
            .additional_claims()
            .groups_direct
            .contains(&users.admin_group),
        hue: rand::random_range(0..360),
    };
    info!("Authenticated user: {user:?}");

    users
        .client
        .revoke_token(CoreRevocableToken::AccessToken(
            token_response.access_token().clone(),
        ))
        .map_err(|e| err(Some(&e), "Failed to revoke access token"))?
        .request_async(&users.http_client)
        .await
        .map_err(|e| err(Some(&e), "Failed to contact revocation endpoint"))?;

    users.sessions.retain(|_, state| match state {
        AuthState::LoggingIn { expires_at, .. } => *expires_at > Instant::now(),
        AuthState::LoggedIn { expires_at, .. } => *expires_at > Instant::now(),
    });
    users.sessions.insert(
        session.clone(),
        AuthState::LoggedIn {
            user: user.clone(),
            expires_at: Instant::now() + Duration::from_secs(LOGGEDIN_EXPIRE_SEC),
        },
    );

    info!(
        "Login successful -> {:?}",
        redirect.as_ref().map(|r| r.as_ref())
    );

    Ok(redirect_to_id(&redirect).into_response())
}

pub async fn logout(
    State(users): State<Arc<UserSessions>>,
    session: Session,
    Query(query): Query<RedirectQuery>,
) -> Result<impl IntoResponse, AppError> {
    users.sessions.remove(&session);
    users.sessions.retain(|_, state| match state {
        AuthState::LoggingIn { expires_at, .. } => *expires_at > Instant::now(),
        AuthState::LoggedIn { expires_at, .. } => *expires_at > Instant::now(),
    });
    Ok(session
        .delete_cookie(redirect_to_id(&query.redirect))
        .into_response())
}

fn redirect_to_id(redirect: &Option<Identifier>) -> impl IntoResponse {
    let redirect_url = if let Some(redirect) = redirect {
        format!("/#{redirect}")
    } else {
        format!("/")
    };
    Html(format!(
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
    ))
}

/// Teach openidconnect about an extension to the OpenID Discovery response
/// that we can use as the RFC 7009 OAuth 2.0 Token Revocation endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RevokationProviderMetadata {
    revocation_endpoint: RevocationUrl,
}
impl AdditionalProviderMetadata for RevokationProviderMetadata {}

type ProviderMetadataWithRevocation = ProviderMetadata<
    RevokationProviderMetadata,
    CoreAuthDisplay,
    CoreClientAuthMethod,
    CoreClaimName,
    CoreClaimType,
    CoreGrantType,
    CoreJweContentEncryptionAlgorithm,
    CoreJweKeyManagementAlgorithm,
    CoreJsonWebKey,
    CoreResponseMode,
    CoreResponseType,
    CoreSubjectIdentifierType,
>;

#[allow(dead_code)]
#[derive(Debug, Deserialize, Serialize)]
struct GitLabClaims {
    groups: Vec<String>,
}

impl AdditionalClaims for GitLabClaims {}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
struct GitLabTokenClaims {
    groups_direct: Vec<String>,
}
impl AdditionalClaims for GitLabTokenClaims {}

type GitLabIdTokenClaims = IdTokenClaims<GitLabTokenClaims, CoreGenderClaim>;

type GitLabIdTokenFields = IdTokenFields<
    GitLabTokenClaims,
    EmptyExtraTokenFields,
    CoreGenderClaim,
    CoreJweContentEncryptionAlgorithm,
    CoreJwsSigningAlgorithm,
>;
