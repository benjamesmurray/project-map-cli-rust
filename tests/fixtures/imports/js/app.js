import * as Sentry from "@sentry/node";
import Stripe from "stripe";
import posthog from "posthog-js";
import { formatAmount } from "./util.js";
import config from "../ts/shared";

export class Checkout {
  constructor(key) {
    this.stripe = new Stripe(key);
    Sentry.init({ dsn: config.dsn });
  }

  total(cents) {
    posthog.capture("total", { cents });
    return formatAmount(cents);
  }
}

export function start() {
  return new Checkout(config.key);
}
