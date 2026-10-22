# Imports fixture

Source tree for the package-import query, Rust `use` resolution, blast-radius depths and the JavaScript skip rules. It is indexed as a whole; paths below are relative to this directory. `common::copy_fixture` copies it to a temp directory, `common::git_fixture` also makes that copy a Git repository with one commit.

| File | Exercises |
|:--|:--|
| `js/app.js` | ES `import` of `@sentry/node`, `stripe`, `posthog-js`; relative imports with and without extension (`./util.js`, `../ts/shared`) |
| `js/legacy.cjs` | `require("x")` for `stripe` and `@sentry/node/tracing` (scoped package keeps two segments); relative `require` |
| `js/lazy.mjs` | `export … from "@sentry/react"`; dynamic `import("posthog-js")` |
| `js/view.jsx` | JSX; `@sentry/react`; a package with no `/`; relative import |
| `js/util.js` | Relative-import target; symbols from a JavaScript file |
| `js/vendor.min.js` | Skipped: named `*.min.*` |
| `js/bundle.js` | Skipped: a line longer than 1,000 characters in the first 32 KB (it imports `stripe`, which must not be reported) |
| `ts/client.ts` | TypeScript: `@sentry/react`, `require("stripe")`, dynamic `import("posthog-js")`, relative import |
| `ts/shared.ts` | Relative-import target |
| `python/app.py` | `import json, sentry_sdk` (two records on one line); `sentry_sdk.integrations`; `sentry_sdk.integrations.django`; relative `from . import helpers` |
| `python/helpers.py` | Relative-import target |
| `php/Billing.php` | Namespaced `use Stripe\StripeClient;`, `Sentry\State\Scope`, `App\Support\Money` |
| `diamond/` | `a` imports `target` and `m2`; `m2` imports `m1`; `m1` imports `target`. `a` is reachable from `target` at depths 1 and 3 and must be reported at 1; `m1` at 1, `m2` at 2 |
| `rust_crate/` | Own `Cargo.toml` (crate `fixture_crate`) |
| `rust_crate/src/lib.rs` | `extern crate serde_json;`, `use serde_json::Value;` |
| `rust_crate/src/user.rs` | `use crate::a::b::Sym;` (symbol), `use crate::a::{b, c};` (grouped, each leaf a module file), `use std::collections::HashMap;` (`std` is a package) |
| `rust_crate/src/a/mod.rs` | `use self::b::Sym;`, `use super::c::Gamma;` |
| `rust_crate/src/a/b.rs` | `use crate::c::Gamma;` (resolves to `src/c/mod.rs`) |
| `rust_crate/src/errors.rs` | Two `impl From<…> for E` blocks: the qualified name `E.from` is not unique |
| `rust_crate/tests/uses_crate_name.rs` | `use fixture_crate::…` (the crate's own name has no package) and `use serde_json::Value` |

Not committed: a JavaScript file larger than 256 KB, which `common::write_large_js` writes into a copy.
