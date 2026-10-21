---
id: 5
title: Retention window
status: proposed
date: 2026-10-02
domain: audit
tags: retention
supersedes:
  - ADR-002
  - ADR-003
---

# ADR-005: Retention window

## Decision

`RetentionPolicy` in `src/audit/retention.rs` keeps 30 days of **ledger** data.

---

The line above is a thematic break, not frontmatter.

## Status

The Audit Service applies it nightly.
