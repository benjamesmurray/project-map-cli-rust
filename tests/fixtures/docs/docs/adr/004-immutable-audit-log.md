---
id: ADR-004
title: Audit logs are immutable
status: accepted
date: "2026-10-01"
domain: audit
tags: [audit, compliance]
supersedes: ADR-001
owner: someone-ignored
---

# ADR-004: Audit logs are immutable

## Context

The Compliance Officer requires that the **ledger** never changes.

## Decision

`AuditLog::append` is the only writer, implemented in `src/audit/log.rs::append`.
Rotation uses `rotate_segments`.

## Consequences

The Casting Director can cite this record.
