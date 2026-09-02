const BUNDLER: string | undefined;
const VERSION: string | undefined;

declare module "*.md" {
  const url: string;
  export default url;
}

declare module "*.css";
