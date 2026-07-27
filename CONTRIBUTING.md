# Contributing

Thank you for contributing to Chat2DB Rust.

## Development setup

Clone the repository with its pinned Community submodule:

```bash
git clone --recurse-submodules https://github.com/OtterMind/Chat2DB-Rust.git
cd Chat2DB-Rust
```

The required toolchains are Rust 1.88 or newer, Java 17, Node.js 22.12 or
newer within the Node 22 release line, and npm 10.9.7.

## Before opening a pull request

Keep changes focused and include tests for changed behavior. Run the complete
local verification gate before submission:

```bash
make verify
```

When changing the external API contract, regenerate and verify the checked-in
OpenAPI and TypeScript artifacts:

```bash
make generate-contracts
make check-contracts
```

Do not commit credentials, local database files, generated build output, or
driver JARs. JDBC drivers remain external verified driver packs.

## License

Unless stated otherwise before submission, contributions are accepted under
the terms in [`LICENSE`](LICENSE), including the contribution terms in Section
6.
