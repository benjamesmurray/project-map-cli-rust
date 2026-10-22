# Ambiguous imports fixture

Python tree for the suffix-match fallback (Strategy 3) of import resolution. File FQNs drop the extension, so `src/app/config.py` is `src.app.config`. Paths below are relative to this directory.

| File | Exercises |
|:--|:--|
| `src/app/service.py` | `import app.config`. Tier 1 has two candidates: `src.app.config` (1 extra leading segment) and `tests.fixtures_copy.app.config` (3). The nearer one wins |
| `src/app/config.py` | Target of the import above |
| `tests/fixtures_copy/app/config.py` | The farther tier-1 candidate; must not be linked |
| `src/app/tie.py` | `import shared.util`. Candidates `lib_a.shared.util` and `lib_b.shared.util` tie on tier and leading segments, so the import stays unlinked and counts as ambiguous |
| `lib_a/shared/util.py`, `lib_b/shared/util.py` | The two tied candidates |
| `src/app/cache.py` | `import redis`. `src/myredis.py` ends in the same letters but not on a segment boundary, so nothing links |
| `src/myredis.py` | The raw-string near miss |
| `src/app/tier2.py` | `import pkg.mod.Thing`. Tier 2: the import starts with the FQN `pkg.mod` |
| `pkg/mod.py` | Target of the tier-2 import |
| `src/app/tier3.py` | `import ext.lib3.mod3`. Tier 3: only the last two segments match `vendor.lib3.mod3` |
| `vendor/lib3/mod3.py` | Target of the tier-3 import |

## Expected `Imports` edges (file to file)

```
src/app/service.py -> src/app/config.py
src/app/tier2.py   -> pkg/mod.py
src/app/tier3.py   -> vendor/lib3/mod3.py
```

Unresolved (`resolved: false` in `imports`): `src/app/tie.py` (`shared.util`, ambiguous) and `src/app/cache.py` (`redis`, no candidate). Ambiguous count: 1.
