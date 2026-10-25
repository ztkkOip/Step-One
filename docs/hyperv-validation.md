# Hyper-V provider verification

Status: **real Hyper-V verification has not been run**. The implementation is a
low-level provider, not enabled as the desktop's default runtime. Passing mock
tests does not establish a working VM, a sandbox, or completion of Issue #5.

## Current automated evidence

`Test-ManageStepOneVm.ps1` invokes the actual protocol/control-flow functions
with in-memory Hyper-V cmdlets. It does not load Hyper-V, create resources,
elevate, enable Windows features, or alter system settings. It checks strict
request fields, UUID ownership, private paths, fixed limits, offline creation,
start/stop/reset/delete, foreign resource refusal, and cleanup when the base
image is unavailable. Run it in a fresh, non-elevated process:

```powershell
powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File ./Test-ManageStepOneVm.ps1
```

The provider accepts the reviewed JSON protocol on stdin. It never accepts
PowerShell text, workspace paths, agent names, shell arguments, or arbitrary
VM settings. stdout is one safe JSON response. A failed command uses exit code
1 and a stable error code; exception messages and host command output are not
returned. Timeout cleanup belongs to the host runner's kill-on-close Job.

## Preconditions for future real verification

- A dedicated Windows 11 Pro/Enterprise test machine, with hardware
  virtualization, Hyper-V module and VMMS already enabled by its owner.
- An isolated management-helper session using the same Windows account as the
  desktop; do not run the entire desktop application as administrator.
- The production elevation/IPC authentication helper is a separate required
  follow-up. The current normal-user runner returns prerequisite failure.
- A reviewed, immutable, UEFI-bootable Linux base VHDX with Secure Boot support,
  virtual capacity exactly 32 GiB, and independently recorded SHA-256. No
  difference image may be used as the base. The provider does not download or
  build an image automatically.
- Image stored outside `%LOCALAPPDATA%\com.ztkk.stepone\hyperv`; no path component
  may be a junction, symbolic link, cloud placeholder, UNC path or device path.
- An expendable test resource UUID. Its VM name must be `step-one-<uuid>`.
  Private resource root is fixed to the Windows account's LocalApplicationData
  folder followed by `com.ztkk.stepone\hyperv`; no other root is accepted.

## Real lifecycle acceptance matrix

Record OS build, account mode, Hyper-V version, image digest, each request's
resource UUID, safe JSON response, elapsed time, VM identity, and before/after
resource inventory. Do not include credentials in evidence.

| Scenario | Required observation | Current evidence |
| --- | --- | --- |
| Standard-user invocation | Prerequisite failure; no elevation or mutation | Not yet recorded against real provider |
| Probe | Module, service and helper privilege detected; missing image does not block cleanup availability | Mocked only |
| Create | Generation 2, 2 vCPU, fixed 2 GiB RAM, 32 GiB differencing disk, Secure Boot Linux template, no network adapter, auto-start/checkpoints disabled | Mocked only |
| Start | Known image hash/parent, limits and network policy checked; VM reaches Running | Mocked only |
| Stop | VM reaches Off; repeated stop remains safe | Mocked only |
| Reset | Refused while running; stopped VM gets a new empty differencing disk; workspace files never touched | Mocked only |
| Delete | Refused while running; only verified VM and session disk removed; unknown files are preserved and reported | Mocked only |
| Foreign resource | Same-name VM with wrong Notes, wrong root/disk or changed settings is refused without mutation | Mocked only |
| Interrupted create | Private disk/ownership retained for diagnosis; VM without matching ownership Notes is never auto-deleted | Mocked only |
| Timeout | Runner terminates helper and descendants; host retains operation/ownership record and re-inspects VM state | Runner mock tests only |
| Missing/changed base image | Create/reset/start denied; inspect/stop/delete still available | Mocked only |
| Reparse paths | Root, image and resource component junctions are refused; cleanup never recursively follows links | Contract tests plus future real filesystem verification |

## Boundaries still outstanding

The provider deliberately removes all VM NICs. This is not a complete agent
sandbox: guest service provisioning, Hyper-V socket authentication, workspace
file broker, handle-relative file access, model/credential broker, controlled
egress, guest Agent installation, and a host-enforced session lifetime still
need their own implementation and validation. `timeoutSeconds = 120` limits
one management operation; it does **not** limit a VM session after startup.

Workspaces are not mounted by this provider. Consequently the planned tests
for authorized workspace read/write, sensitive host-directory rejection from
inside the guest, and guest credential isolation remain unperformed. A VM
running is not evidence that an Agent is installed or running.

Known recovery constraint: Hyper-V's creation and setting VM Notes are separate
operations. An interruption between them can leave a VM without the ownership
marker. The provider fails closed for that VM and requires a human administrator
to inspect and reconcile it. Cleanup also refuses unknown remaining files.
Neither case should be reported as a successful deletion or automatically
discarded from the host's resource ledger.

Do not close Issue #5 or #6, advertise real isolation, or enable this provider by
default until the required real-machine evidence and dependent controls exist.
