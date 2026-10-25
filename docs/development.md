# Development guide

## Prerequisites

- Node.js 20 or newer
- Rust stable and Cargo
- Windows 11 Pro or Enterprise for the future Hyper-V runtime
- WebView2 for the Windows desktop build

The current foundation can be previewed in a browser and does not require Hyper-V.

## Start the frontend preview

```bash
cd frontend
npm install
npm run dev
```

Open `http://127.0.0.1:5173` to preview the simulator UI. Browser preview keeps state in memory and catches Tauri command failures so the main workflow remains visible.

## Start the Tauri desktop app

```bash
cd frontend
npm install
npm run tauri dev
```

## Build checks

```bash
cd frontend
npm run typecheck
npm run build
cargo check --manifest-path src-tauri/Cargo.toml
```

## Runtime modes

- `simulator`: current development mode. It models Agent lifecycle and permission state in memory.
- `hyperv`: reserved for the real Windows sandbox runtime in the dedicated runtime PR.

The simulator must never be described as a security boundary. The real runtime must be enabled only after its workspace, network, credential and cleanup controls are validated on Windows 11 Pro/Enterprise.
