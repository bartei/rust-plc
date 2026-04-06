# Contributing to rust-plc

Thank you for your interest in contributing to rust-plc! This project is an open-source IEC 61131-3 Structured Text compiler toolchain, and we welcome contributions from the community.

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [How to Contribute](#how-to-contribute)
- [Development Setup](#development-setup)
- [Pull Request Process](#pull-request-process)
- [Coding Standards](#coding-standards)
- [Testing Requirements](#testing-requirements)
- [Commit Convention](#commit-convention)
- [License](#license)

## Code of Conduct

This project follows the [Contributor Covenant Code of Conduct](https://www.contributor-covenant.org/version/2/1/code_of_conduct/). By participating, you are expected to uphold this code. Please report unacceptable behavior by opening an issue.

## How to Contribute

### Reporting Bugs

- Search [existing issues](https://github.com/bartei/rust-plc/issues) before creating a new one
- Use the bug report template and include:
  - A minimal `.st` file that reproduces the issue
  - Expected vs actual behavior
  - `st-cli --version` output
  - Steps to reproduce

### Suggesting Features

- Open a [discussion](https://github.com/bartei/rust-plc/discussions) for feature ideas
- Reference the IEC 61131-3 spec section if applicable
- Describe the use case, not just the solution

### Submitting Code

1. Fork the repository
2. Create a feature branch from `master`
3. Make your changes with tests
4. Submit a pull request

## Development Setup

### Prerequisites

- Rust 1.85+ (`rustup install stable`)
- Node.js 18+ (for VSCode extension)
- tree-sitter CLI (`cargo install tree-sitter-cli`)

### Building

```bash
git clone https://github.com/bartei/rust-plc.git
cd rust-plc
cargo build --workspace
```

### Running Tests

```bash
# All tests (510+)
cargo test --workspace

# Specific crate
cargo test -p st-semantics

# With coverage
cargo llvm-cov --workspace --html
```

### Using the Devcontainer

The easiest way to get a full development environment:

1. Open the repo in VSCode
2. Click "Reopen in Container"
3. Everything is pre-configured (Rust, Node.js, extension installed)

### Extension Development

Press **F5** in VSCode to launch the Extension Development Host with the playground folder open. This builds both `st-cli` and the TypeScript extension automatically.

## Pull Request Process

1. **Branch from `master`** — create a descriptive branch name (e.g., `feat/array-bounds-checking`, `fix/timer-reset-bug`)

2. **Write tests** — every new feature or bug fix must include tests:
   - Unit tests in the relevant crate
   - Integration tests for cross-crate behavior
   - LSP protocol tests for language server features
   - End-to-end tests for user-facing behavior

3. **Run the full test suite** before submitting:
   ```bash
   cargo test --workspace
   cargo clippy --workspace --all-targets -- -D warnings
   ```

4. **Update documentation** — if you add or change user-facing behavior:
   - Update relevant pages in `docs/src/`
   - Run `cd docs && mdbook build` to verify
   - Update `todo.md` if completing a planned feature

5. **Open a pull request** with:
   - A clear title using [conventional commits](#commit-convention)
   - A description of what changed and why
   - Reference to any related issues

6. **CI must pass** — the PR will be checked for:
   - `cargo check` — compilation
   - `cargo test` — all tests pass
   - `cargo clippy` — no lint warnings
   - `cargo audit` — no security vulnerabilities
   - `cargo-deny` — dependency license and advisory checks
   - `mdbook build` — documentation builds

## Coding Standards

### Rust

- Follow standard Rust idioms and the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- All public APIs must have doc comments
- Use `thiserror` for error types, `tracing` for logging
- No `unsafe` without a safety comment and a compelling reason
- Prefer `clippy::pedantic` compliance

### Structured Text

- Standard library modules in `stdlib/` should follow IEC 61131-3 naming conventions
- Use uppercase keywords (`PROGRAM`, `IF`, `END_IF`)
- Timer inputs are named `IN1` (not `IN`) to avoid keyword conflicts
- Include a comment header in every stdlib file describing the module

### TypeScript (VSCode extension)

- Follow the [VSCode Extension Guidelines](https://code.visualstudio.com/api/references/extension-guidelines)
- Use strict TypeScript (`"strict": true` in tsconfig)

## Testing Requirements

This project maintains 510+ tests across 10 crates. New contributions must include tests:

| Area | Test Type | Location |
|------|-----------|----------|
| Grammar | Parser tests | `crates/st-grammar/src/lib.rs` |
| AST/Syntax | Lowering tests | `crates/st-syntax/tests/` |
| Semantics | Diagnostic tests | `crates/st-semantics/tests/` |
| Compiler | Bytecode tests | `crates/st-compiler/tests/` |
| Runtime | VM execution tests | `crates/st-runtime/tests/` |
| LSP | Protocol integration tests | `crates/st-lsp/tests/` |
| DAP | Debug protocol tests | `crates/st-dap/tests/` |
| Monitor | WebSocket tests | `crates/st-monitor/tests/` |
| Project | Discovery tests | `crates/st-syntax/src/project.rs` |
| Stdlib | End-to-end stdlib tests | `crates/st-runtime/tests/stdlib_tests.rs` |

### Writing a Good Test

```rust
#[test]
fn descriptive_test_name() {
    let source = r#"
PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    x := x + 1;
END_PROGRAM
"#;
    let engine = run_program(source, 10);
    assert_eq!(engine.vm().get_global("result"), Some(&Value::Int(10)));
}
```

## Commit Convention

We use [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <description>

[optional body]
```

**Types:**

| Type | When |
|------|------|
| `feat` | New feature |
| `fix` | Bug fix |
| `docs` | Documentation only |
| `test` | Adding or updating tests |
| `refactor` | Code change that doesn't fix a bug or add a feature |
| `ci` | CI/CD changes |
| `chore` | Maintenance tasks |

**Scopes:** `grammar`, `syntax`, `semantics`, `ir`, `compiler`, `runtime`, `lsp`, `dap`, `monitor`, `cli`, `stdlib`, `vscode`, `docs`

**Examples:**
```
feat(stdlib): add SHL/SHR bit shift functions
fix(debugger): breakpoint not triggering on line 1
docs(tutorial): add section on multi-file projects
test(runtime): add timer precision tests
```

## License

By contributing to rust-plc, you agree that your contributions will be licensed under the **GNU General Public License v3.0** (GPL-3.0).

This means:
- Your contribution will be freely available under the same license
- Anyone can use, modify, and distribute it, provided derivative works remain GPL-3.0
- You retain copyright of your contributions
- You certify that you have the right to submit the contribution under this license

See the [LICENSE](LICENSE) file for the full GPL-3.0 text.

---

Thank you for helping make rust-plc better!
