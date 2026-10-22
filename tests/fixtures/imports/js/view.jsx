import React from "react";
import { ErrorBoundary } from "@sentry/react";
import { formatAmount } from "./util";

export function Price({ cents }) {
  return (
    <ErrorBoundary>
      <span>{formatAmount(cents)}</span>
    </ErrorBoundary>
  );
}
