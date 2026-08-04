# BitVanes — Proprietary License

Copyright (c) BitVanes. All rights reserved.

**This source code is proprietary and is NOT open-source.**

## Grant

The BitVanes engine (the `core/`, `cli/`, `nerd/`, and `web/` components in
this repository) is **free to use for internal data purification** — you may
run it, internally, to detect, redact, and sanitize sensitive data on your own
hardware and for your own data.

## Restrictions

You may **not**, without prior written permission from the author:

- **Redistribute** the source code, or derivative works of it, to third parties;
- **Host** BitVanes as a service offered to others (internal use only);
- **Remove or alter** this license notice, the entitlement/signature machinery,
  or the provenance/SHA records for the bundled model artifact.

Compiled release binaries downloaded from the project's official releases are
distributed under separate terms accompanying those releases.

## Tier-2 NER model — separate license

The bundled NER model under `nerd/models/` (`Xenova/bert-base-NER`, a
pre-exported ONNX copy of `dslim/bert-base-NER`) is **not** covered by this
license. It retains its upstream **Apache-2.0** license; see
`nerd/models/MANIFEST.toml` for provenance, the pinned revision, and SHA-256.
Likewise, all third-party dependencies (Rust crates, `libonnxruntime`,
`libpdfium`, npm packages) retain their respective licenses.

## No warranty

THIS SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED. The author is not liable for any claim, damages, or other liability
arising from the use of the engine, including data that passes through it. The
PII detection surface is a best-effort control, not a guarantee of complete
redaction — see `core/SECURITY.md` for the recall scope.

## Contact

For licensing inquiries, commercial use beyond internal data purification,
redistribution, or to obtain a paid entitlement key: contact the author via
the project repository at https://github.com/BitVanes/BitVanes.
