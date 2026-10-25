# Development

Use Node.js 20+ and Rust stable. The standalone control-plane crate runs on Windows/Linux without Tauri or Hyper-V. Windows desktop compilation additionally needs MSVC and WebView2. Real Hyper-V acceptance needs Windows 11 Pro/Enterprise.

## Frontend

```sh
cd frontend
npm ci
npm run dev
npm run typecheck
npm run test:coverage
npm run build
npm run tauri dev
```

Browser preview is explicitly simulated. Desktop errors never fall back to preview success. Use the native directory picker to authorize desktop workspaces.

## Rust (repository root)

```sh
cargo fmt --manifest-path crates/control-plane/Cargo.toml --check
cargo clippy --manifest-path crates/control-plane/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path crates/control-plane/Cargo.toml --locked
cargo check --manifest-path frontend/src-tauri/Cargo.toml --locked
```

Install cargo-llvm-cov and llvm-tools-preview for Linux coverage:

```sh
cargo llvm-cov --manifest-path crates/control-plane/Cargo.toml --all-targets --summary-only --fail-under-lines 90
```

CI runs frontend tests/build, Rust tests/coverage and a Windows desktop compile check. Frontend thresholds: lines/statements >=90%, branches >=85%, functions >=90%. Rust line coverage targets >=90%. Rust branch coverage requires experimental compiler instrumentation and is not inferred from line coverage.

## State and collaboration

Desktop stores control.sqlite3 in Tauri's application-local data directory. A second instance using the same database fails safely. Preserve database files on errors; restart to reconcile interrupted operations. Browser preview stays ephemeral.

main contains reviewed work; zt is the teammate branch; ll is our integration branch. Focused PRs target ll and reference an Issue with matching labels. Stage PRs deliver ll to main. Preserve real authors and incremental commits. Do not close #5/#6 based on simulated tests.

## Read-only host diagnostics

The Connections view and runtime_capabilities command report Windows edition, virtualization, Hyper-V module, VM service, management access and image prerequisites. This diagnostic does not start VMs or change system settings. STEP_ONE_HYPERV_IMAGE may name an absolute VHDX file; finding it is not verification of checksum, boot compatibility or isolation. Desktop lifecycle remains simulated and persisted by ControlService until real-runtime acceptance.
