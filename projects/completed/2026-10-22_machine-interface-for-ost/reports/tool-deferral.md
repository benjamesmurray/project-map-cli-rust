# Claude Code tool deferral for the `map` MCP server (task 1.5)

Date: 2026-10-22. Claude Code 2.1.308. Source: <https://code.claude.com/docs/en/mcp>, sections "Scale with MCP tool search" and "Configure tool search" (read in full), plus this session's own evidence.

## Answer

**Claude Code defers the `map` tools by default.** Tool search is on by default: every MCP tool is deferred and found through `ToolSearch`. At session start Claude sees only the tool *names* and the server's *instructions*. This matches what the 2026-10-22 session saw: `mcp__map__*` listed by name only, loadable through ToolSearch. The docs say "Only tool names and server instructions load at session start".

`ENABLE_TOOL_SEARCH` is unset in this environment, as are `CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS` and `MCP_*`. The `map` server is registered user-wide in `~/.claude.json` (`type: stdio`, `project-map-cli-rust mcp -w`) with no `alwaysLoad`.

## Settings that change it

| Who | Setting | Effect |
|:--|:--|:--|
| User, per server | `"alwaysLoad": true` in the server's MCP config entry (`~/.claude.json` or `.mcp.json`; all server types; v2.1.287+ for the `false` form) | All of the server's tools load into context at session start, whatever `ENABLE_TOOL_SEARCH` says. Startup waits for the server's tools, up to the 5 s connect timeout |
| User, global | `ENABLE_TOOL_SEARCH=false` | Every MCP tool of every server loads upfront |
| User, global | `ENABLE_TOOL_SEARCH=auto` or `auto:N` | Tools load upfront while all deferred definitions together stay under 10 % (or N %) of the context window; deferral starts above it |
| Server author, per tool | `"_meta": {"anthropic/alwaysLoad": true}` on a `tools/list` entry (v2.1.285+) | That tool loads upfront without the user changing anything. The user can still defer it with `"alwaysLoad": false` |
| Server author | `instructions` in the initialize result | Shown to the model at session start even while the tools are deferred; the docs recommend it as the way to tell Claude *when to search for your tools* |

Not affected by server-side tool count or annotations: the docs describe no tool-count cap and no annotation that changes deferral. `CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS` forces tool search off and cannot be overridden by `ENABLE_TOOL_SEARCH`.

Limits that matter for what we write (defaults; `CLAUDE_CODE_MAX_MCP_DESCRIPTION_LENGTH` replaces both):
- Server instructions and the description of an upfront tool: **4,096 characters**; text past it is cut.
- A tool description loaded through tool search: 16,384 characters, but matching reads only the first 4,096, so the words Claude would search for must come first.

## Implications for 4.2 (server instructions and tool descriptions)

1. **Instructions are the one channel that always reaches the model.** The 0.3.0 server sends none, so a deferred `map` is invisible to an agent that does not think to search for it. 4.2 adds instructions: under about 1,500 characters (well inside 4,096), starting with what the index covers and "use these tools instead of grep for symbols, dependents, imports and docs", then the tool names with a few words each, so a ToolSearch for any of those words finds the tools.
2. **Front-load tool descriptions.** Put "when to use instead of grep" in the first sentence of every description (the 4,096-character window is what search reads). Existing descriptions already lead with this; the two new tools (`modules`, `imports`) follow it.
3. **Per-tool `anthropic/alwaysLoad` is possible but is the owner's call.** The SDK's `Tool` struct has a `meta` map (rust-mcp-schema 0.10.3), so a server can mark a few tools (for example `status` and `search`) to load upfront. That costs context on every session of every client and overrides what the user chose, so 4.2 does **not** set it. It is listed in the README as an option and left to the owner.
4. **The format argument costs deferred-tool description size.** Every tool gains `format`; keep that description to one sentence.

## What the owner would change (nothing was changed here)

To have the `map` tools present at session start, either:
- add `"alwaysLoad": true` to the `map` entry in `~/.claude.json` (best: only this server, and the tool list is small); or
- set `ENABLE_TOOL_SEARCH=auto:5` (or `false`) in the environment or `settings.json` `env` block, if every MCP server should load upfront.

Without either, rely on the server instructions (4.2) to make the model search for the tools.

## Not verified

The "quick experiment in a fresh session" was not repeated: this very session already shows the deferred state (tool names only, empty server instructions) against the documented default, and the documented settings were read, not trialled. Whether `alwaysLoad: true` makes the tools appear as callable at session start is as documented but untested; test it once after the owner applies it, by checking that `mcp__map__status` is callable without a ToolSearch call.
