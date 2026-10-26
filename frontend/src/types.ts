export type Provider = 'Claude Code' | 'Codex' | 'DeepSeek Harness'
export type RuntimeCheckStatus = 'passed' | 'blocked' | 'unknown'
export type RuntimeCheck = { id: string; status: RuntimeCheckStatus; message: string }
export type RuntimeCapabilities = { mode: 'hyperv' | 'simulator'; available: boolean; checks: RuntimeCheck[] }

export type PermissionKey = 'workspaceWrite' | 'network' | 'mcpTools' | 'clipboard' | 'exportFiles'
export type Permissions = Record<PermissionKey, boolean>
export type AgentAction = 'start' | 'stop'
export type AgentStatus = 'ready' | 'running' | 'stopped' | 'unknown'
export type RuntimeKind = 'simulator' | 'hyperv'
export type RuntimeState = 'creating' | 'ready' | 'starting' | 'running' | 'stopping' | 'stopped' | 'resetting' | 'deleting' | 'failed' | 'missing' | 'updating'
export type ControlError = { code: string; message: string; retryable: boolean; operationId: string | null }
export type RuntimeInfo = { kind: RuntimeKind; available: boolean; isolationEnforced: boolean; capabilities: Permissions; message: string }
export type WorkspaceGrant = { id: string; path: string }
export type Agent = {
  id: string
  name: string
  provider: Provider
  version: string
  status: AgentStatus
  workspace: string
  securityMode: 'Protected' | 'Developer'
  permissions: Permissions
  lastActivity: string
  accent: string
  runtimeKind: RuntimeKind
  runtimeState: RuntimeState
  operationId: string | null
  lastError: ControlError | null
  resourceId: string
}
export type CreateAgentInput = Pick<Agent, 'name' | 'provider' | 'securityMode'> & { workspaceGrantId: string; runtimeKind?: RuntimeKind }
export type AuditEvent = { id: string; time: string; title: string; detail: string }
export type Approval = { id: string; agentId: string; action: string; title: string; detail: string }
export type Operation = { id: string; agentId: string; action: string; state: 'pending' | 'succeeded' | 'failed'; error: ControlError | null }
export type Snapshot = { agents: Agent[]; events: AuditEvent[]; approvals: Approval[]; runtime: RuntimeInfo; operations: Operation[] }

export const permissionLabels: Record<PermissionKey, string> = {
  workspaceWrite: '修改工作区', network: '访问网络', mcpTools: '调用 MCP 工具',
  clipboard: '读取剪贴板', exportFiles: '导出文件',
}
export const permissionDescriptions: Record<PermissionKey, string> = {
  workspaceWrite: '允许修改所选目录中的文件。', network: '允许向外部服务发送数据，可能包含工作区内容。',
  mcpTools: '允许通过配置的工具访问外部服务。', clipboard: '剪贴板可能包含私人信息。',
  exportFiles: '允许将工作成果导出到工作区之外。',
}
export const deniedPermissions = (): Permissions => ({ workspaceWrite: false, network: false, mcpTools: false, clipboard: false, exportFiles: false })
export const pendingRuntimeStates: RuntimeState[] = ['creating', 'starting', 'stopping', 'resetting', 'deleting', 'updating']
export const agentPending = (agent: Agent): boolean => agent.operationId !== null || pendingRuntimeStates.includes(agent.runtimeState)
