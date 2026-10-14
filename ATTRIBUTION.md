# Attribution

Every asset and third-party program in this repository or its Docker image is listed here.

RemoteCraft's code, documentation and demo desk (`crates/core/src/mock.rs`, every name, ID and
note invented) are by the RemoteCraft contributors (MIT OR Apache-2.0). The repository ships no
image, font or icon files.

| What | Author | Source | Licence | How it is used |
|---|---|---|---|---|
| `hbbs`, `hbbr` (RustDesk Server OSS) | Purslane Ltd. and the RustDesk contributors | the official image `rustdesk/rustdesk-server`, source at https://github.com/rustdesk/rustdesk-server | AGPL-3.0 | copied unmodified into the Docker image at build time; RemoteCraft runs them as separate processes, talks to them only over the network and reads their public key file. No RustDesk code is in this repository. |

RustDesk's client link format (`rustdesk://connection/new/<id>`), command line
(`rustdesk --connect <id>`), import config string and installer naming
(`rustdesk-host=…,key=…,.exe`) are interfaces of the RustDesk client, described in its public
documentation.
