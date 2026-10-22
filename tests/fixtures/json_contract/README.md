# JSON contract goldens

Each `<tool>[_<case>].json` file is the exact JSON envelope one tool returns for a fixed
call on the docs fixture (`tests/fixtures/docs`) or the imports fixture
(`tests/fixtures/imports`). `tests/json_contract_test.rs` builds both fixtures into
fixed-name directories, runs every call and compares the result with its file. A file
without a case, a case without a file, an added, removed or renamed field, or a changed
value fails the test and prints a line diff.

Machine- and build-specific fields are replaced before comparison: `root`, `built_at`,
`tool_version`, and the Git SHAs (`commit`, `head_now`) become `<root>`, `<built_at>`,
`<tool_version>` and `<sha>`.

## Changing a golden is a breaking change

The JSON envelope, its field names and its error codes are a public contract (the
`contract` number in every envelope). Before you regenerate any file:

1. Decide whether the change breaks a program that reads the JSON. Adding, removing or
   renaming a field, changing a value's type, or changing an error code does.
2. For a breaking change, raise `CONTRACT_VERSION` in `src/core/json_out.rs` (before 1.0 a
   minor version bump is enough) and add a **Breaking** entry to `CHANGELOG.md`.
3. Regenerate with:

   ```
   UPDATE_GOLDENS=1 cargo test --test json_contract_test
   ```

4. Review `git diff tests/fixtures/json_contract/` line by line. Every changed line is a
   change a consumer will see.

A new case (a new tool or option) is added in `tests/json_contract_test.rs` and generated
the same way; adding a new tool is not breaking, but a new field on an existing tool's
item is a contract change to review.
