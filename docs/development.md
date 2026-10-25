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

## Runtime diagnostics

The desktop control plane exposes a read-only prerequisite check through the Connections view and the `runtime_capabilities` Tauri command. It checks the Windows edition, firmware virtualization state, Hyper-V management module, VM management service, current management access, and the configured Linux VHDX path.

Set `STEP_ONE_HYPERV_IMAGE` to an absolute `.vhdx` path when testing the future runtime. Finding a VHDX file does not verify its provenance, checksum, Linux boot compatibility, guest integration, workspace transfer, or cleanup behavior. The runtime remains in simulator mode until those controls are validated on Windows 11 Pro/Enterprise.

The current development host may report blocked prerequisites. That is an expected diagnostic result and must not be bypassed by enabling arbitrary commands or presenting simulator state as VM isolation.
