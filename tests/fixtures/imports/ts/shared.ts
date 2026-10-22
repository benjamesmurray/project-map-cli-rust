export interface Config {
  dsn: string;
  key: string;
}

const config: Config = { dsn: "https://example.invalid/1", key: "sk_test" };
export default config;
