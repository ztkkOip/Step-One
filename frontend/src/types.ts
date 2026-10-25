export type Provider = 'Claude Code' | 'Codex' | 'DeepSeek Harness'
export type PermissionKey = 'workspaceWrite' | 'network' | 'mcpTools' | 'clipboard' | 'exportFiles'
export type Permissions = Record<PermissionKey, boolean>
export type AgentAction = 'start' | 'stop'
export type AgentStatus = 'ready' | 'running' | 'stopped'
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
}
export type CreateAgentInput = Pick<Agent, 'name' | 'provider' | 'workspace' | 'securityMode'>
export type AuditEvent = { id: string; time: string; title: string; detail: string }
export type Approval = {
  id: string
  agentId: string
  action: string
  title: string
  detail: string
}
export type Snapshot = { agents: Agent[]; events: AuditEvent[]; approvals: Approval[] }

export const permissionLabels: Record<PermissionKey, string> = {
  workspaceWrite: '修改工作区', network: '访问网络', mcpTools: '调用 MCP 工具',
  clipboard: '读取剪贴板', exportFiles: '导出文件',
}
export const permissionDescriptions: Record<PermissionKey, string> = {
  workspaceWrite: '允许修改所选目录中的文件。', network: '允许向外部服务发送数据，可能包含工作区内容。',
  mcpTools: '允许通过配置的工具访问外部服务。', clipboard: '剪贴板可能包含私人信息。',
  exportFiles: '允许将工作成果导出到工作区之外。',
}
export const deniedPermissions = (): Permissions => ({
  workspaceWrite: false, network: false, mcpTools: false, clipboard: false, exportFiles: false,
})
