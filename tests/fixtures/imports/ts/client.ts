import * as Sentry from "@sentry/react";
import config, { Config } from "./shared";

const stripe = require("stripe");

export function init(c: Config = config): void {
  Sentry.init({ dsn: c.dsn });
}

export async function lazyAnalytics() {
  const { default: posthog } = await import("posthog-js");
  posthog.capture("loaded");
  return stripe;
}
