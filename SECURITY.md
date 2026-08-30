# Security policy

ART stores bounded provenance and human-reviewed knowledge. It rejects passwords, private keys, access tokens, cookies, authorization headers, recovery codes, full SSH public-key bodies, path traversal, and private-data export without explicit confirmation.

Per-Agent SQLite files provide application-level isolation through ART interfaces. They are not a sandbox against another process running as the same operating-system user with arbitrary filesystem access. Strong hostile-agent isolation requires separate OS identities or an external sandbox.

Report a suspected vulnerability privately to the maintainers. Do not include live secrets or private memory content in a report.

