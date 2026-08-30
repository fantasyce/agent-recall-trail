# DSH integration

Validated against locally installed `dsh 0.1.1-rc.2` on 2026-08-30. The overlay is explicit and temporary; it starts one ART stdio child bound to `ART_AGENT`. DSH exposes the tools as `mcp__art__art_*`, disposes the child on normal shutdown, and owns reconnect/HMR behavior after an abnormal transport close.

```bash
ART_BINARY=/absolute/path/to/art \
ART_HOME=/task-owned/art-home \
ART_AGENT=dsh-primary \
dsh --profile headless --patch integrations/dsh/art.overlay.yml "your task"
```

ART guarantees bounded EOF/signal shutdown and database integrity. DSH owns reconnect attempts and retains tool-call history already delivered to its session. Do not place the ART data root in an Agent workspace when stronger same-user isolation is required.
