# DSH community post

## Title

[MCP toolkit] ART + ARP + ARE — local memory, runtime proof, and residue evidence for DSH

## Body

Maintainer disclosure: I maintain these three open-source projects.

I am looking for sanitized, reproducible feedback from DeepSeek Harness users
on three local reliability boundaries:

1. **ART — Agent Recall Trail** keeps one private memory trail per Agent and
   publishes shared Knowledge Editions only through explicit governance. It
   includes a DSH overlay and Skill.
2. **ARP — Agent Runtime Proof** checks whether a live Agent or MCP process
   matches an explicitly approved artifact. It includes a data-only DeepSeek
   Harness host profile.
3. **ARE — Agent Residue Evidence** records task-scoped files, processes, and
   listening ports before cleanup decisions. It is a generic local stdio MCP
   server and does not currently ship a DSH-specific integration.

These are **DSH-compatible MCP tools, not native DSH plugins**. They are
separately installable, local-first, and do not edit DSH configuration. None of
them uploads code, credentials, or transcripts.

- ART: https://github.com/fantasyce/agent-recall-trail
- ART DSH integration: https://github.com/fantasyce/agent-recall-trail/tree/main/integrations/dsh
- ARP: https://github.com/fantasyce/agent-runtime-proof
- ARP DSH host profile: https://github.com/fantasyce/agent-runtime-proof/blob/main/docs/hosts/deepseek-harness.md
- ARE: https://github.com/fantasyce/agent-residue-evidence

If you try one, the most useful report is: DSH version, operating system,
project version, exact workflow attempted, expected result, observed result,
and sanitized logs or evidence. Please omit credentials, private paths,
proprietary source, and full transcripts.

I would especially value feedback on whether these boundaries fit DSH's MCP
launch and reconnect model, and where the integration guidance is unclear.
