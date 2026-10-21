Before the first heading we mention `src/audit/log.rs`.

# Link Coverage

## Markdown Links

- Relative link with fragment: [audit log](../src/audit/log.rs#L11)
- Relative link to a directory-relative doc: [guide](guide.md#usage)
- External link: [example](https://example.com/src/audit/log.rs)
- Mail link: [mail](mailto:audit@example.com)
- Pure anchor: [top](#link-coverage)
- Leaves the index root: [outside](../../../../Cargo.toml)
- Missing target: [missing](../src/missing.rs)

## Code Spans

### Paths

Plain path `src/audit/retention.rs` and dotted path `./src/billing/invoice.py`.
Missing path `src/audit/nothing.rs`.

### Qualified Symbols

Path-qualified `src/audit/log.rs::append` and line-qualified `src/audit/log.rs:12`.
Line outside any symbol `src/audit/log.rs:1`.
Type-qualified `AuditLog::append` and dotted `AuditLog.new`.
Ambiguous type `RetentionPolicy::apply` exists in two files.

### Bare Names

Unique bare name `rotate_segments`.
Duplicated bare name `flush_buffer`.
Denylisted name `new` and short name `run`.
Unknown bare name `does_not_exist`.

## Fenced Code

```rust
let mut log = AuditLog::new();
verify_chain(&log);
// see src/audit/retention.rs
```

The Casting Director and the Audit Service share this page.
