export { init } from "@sentry/react";

export async function loadAnalytics() {
  const mod = await import("posthog-js");
  return mod.default;
}
