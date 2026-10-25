import { invoke, isTauri } from '@tauri-apps/api/core'
import { deniedPermissions, permissionDescriptions, permissionLabels } from './types'
import type { Agent, AgentAction, CreateAgentInput, PermissionKey, Snapshot, WorkspaceGrant } from './types'

export const isDesktop = isTauri()

// Browser preview is separate from desktop IPC. Desktop failures never fall
// back to simulated success or touch browser state.
export function createPreviewConsole() {
  const preview: Snapshot = {
    agents: [], events: [], approvals: [], operations: [],
    runtime: {
      kind: 'simulator', available: true, isolationEnforced: false,
      capabilities: { workspaceWrite: true, network: true, mcpTools: true, clipboard: true, exportFiles: true },
      message: '模拟模式仅用于流程演示，不运行 Agent，也不提供安全隔离。',
    },
  }
  const grants = new Map<string, string>()
  function record(title: string, detail: string) {
    preview.events.unshift({ id: crypto.randomUUID(), time: String(Date.now()), title, detail })
    preview.events = preview.events.slice(0, 500)
  }
  function snapshot() { return structuredClone(preview) }
  function getAgent(id: string) {
    const agent = preview.agents.find(item => item.id === id)
    if (!agent) throw new Error('Agent 不存在，请刷新。')
    return agent
  }
  function ensureEditable(id: string) {
    if (getAgent(id).status === 'running') throw new Error('请先停止会话。')
    if (preview.approvals.some(item => item.agentId === id)) throw new Error('请先处理待确认请求。')
  }
  return {
    async load(): Promise<Snapshot> { return snapshot() },
    async chooseWorkspace(): Promise<WorkspaceGrant> {
      const grant = { id: crypto.randomUUID(), path: '模拟工作区（不会访问本机文件）' }
      grants.set(grant.id, grant.path)
      return grant
    },
    async create(input: CreateAgentInput): Promise<Snapshot> {
      if (input.runtimeKind && input.runtimeKind !== 'simulator') throw new Error('真实沙箱尚不可用。')
      if (!input.name.trim() || [...input.name.trim()].length > 80) throw new Error('请输入 1–80 个字符的名称。')
      if (!['Claude Code', 'Codex', 'DeepSeek Harness'].includes(input.provider)) throw new Error('不支持的 Agent 类型。')
      if (!['Protected', 'Developer'].includes(input.securityMode)) throw new Error('不支持的安全模式。')
      const workspace = grants.get(input.workspaceGrantId)
      if (!workspace) throw new Error('请重新选择工作区。')
      const accent = input.provider === 'Claude Code' ? 'violet' : input.provider === 'Codex' ? 'blue' : 'orange'
      const agent: Agent = {
        name: input.name.trim(), provider: input.provider, securityMode: input.securityMode,
        id: crypto.randomUUID(), status: 'ready', permissions: deniedPermissions(), workspace,
        version: 'not-installed', lastActivity: '刚刚', accent, runtimeKind: 'simulator', runtimeState: 'ready',
        operationId: null, lastError: null, resourceId: crypto.randomUUID(),
      }
      preview.agents.unshift(agent)
      record('创建模拟 Agent', agent.name)
      return snapshot()
    },
    async action(agentId: string, action: AgentAction): Promise<Snapshot> {
      const agent = getAgent(agentId)
      if (preview.approvals.some(item => item.agentId === agentId)) throw new Error('请先处理待确认请求。')
      if (action === 'start' && (agent.status === 'ready' || agent.status === 'stopped')) agent.status = 'running'
      else if (action === 'stop' && agent.status === 'running') agent.status = 'stopped'
      else throw new Error('当前状态不支持此操作。')
      agent.runtimeState = action === 'start' ? 'running' : 'stopped'
      agent.lastActivity = '刚刚'
      record(action === 'start' ? '启动模拟会话' : '停止模拟会话', agent.name)
      return snapshot()
    },
    async request(agentId: string, action: 'reset' | 'delete' | PermissionKey): Promise<Snapshot> {
      ensureEditable(agentId)
      const agent = getAgent(agentId)
      if (action !== 'reset' && action !== 'delete' && !(action in permissionLabels)) throw new Error('不支持的权限操作。')
      if (action in permissionLabels && agent.permissions[action as PermissionKey]) throw new Error('该权限已经允许。')
      const title = action === 'reset' ? '重置模拟环境' : action === 'delete' ? '删除模拟 Agent' : permissionLabels[action]
      const detail = action === 'reset' ? '撤销所有模拟权限，保留实际工作区文件。'
        : action === 'delete' ? '移除模拟 Agent 配置，保留审计记录和实际工作区文件。'
          : `${permissionDescriptions[action]} 当前仅修改模拟策略。`
      preview.approvals.push({ id: crypto.randomUUID(), agentId, action, title, detail: `${agent.name}：${detail}` })
      record('请求用户确认', `${agent.name} · ${title}`)
      return snapshot()
    },
    async decide(approvalId: string, allowed: boolean): Promise<Snapshot> {
      const approval = preview.approvals.find(item => item.id === approvalId)
      if (!approval) throw new Error('请求已处理或不存在。')
      const agent = getAgent(approval.agentId)
      if (agent.status === 'running') throw new Error('请先停止会话。')
      if (allowed) {
        if (approval.action === 'delete') preview.agents = preview.agents.filter(item => item.id !== agent.id)
        else if (approval.action === 'reset') { agent.status = 'ready'; agent.runtimeState = 'ready'; agent.permissions = deniedPermissions() }
        else agent.permissions[approval.action as PermissionKey] = true
      }
      preview.approvals = preview.approvals.filter(item => item.id !== approvalId)
      record(allowed ? '用户确认' : '用户拒绝', approval.detail)
      return snapshot()
    },
    async revoke(agentId: string, key: PermissionKey): Promise<Snapshot> {
      ensureEditable(agentId)
      if (!(key in permissionLabels)) throw new Error('未知权限。')
      const agent = getAgent(agentId)
      agent.permissions[key] = false
      record('撤销模拟权限', `${agent.name} · ${permissionLabels[key]}`)
      return snapshot()
    },
  }
}

const preview = createPreviewConsole()
export const consoleApi = {
  async load(): Promise<Snapshot> { return isDesktop ? invoke('console_snapshot') : preview.load() },
  async chooseWorkspace(): Promise<WorkspaceGrant | null> { return isDesktop ? invoke('choose_workspace') : preview.chooseWorkspace() },
  async create(input: CreateAgentInput): Promise<Snapshot> { return isDesktop ? invoke('create_agent', { input }) : preview.create(input) },
  async action(agentId: string, action: AgentAction): Promise<Snapshot> { return isDesktop ? invoke('agent_action', { agentId, action }) : preview.action(agentId, action) },
  async request(agentId: string, action: 'reset' | 'delete' | PermissionKey): Promise<Snapshot> { return isDesktop ? invoke('request_approval', { agentId, action }) : preview.request(agentId, action) },
  async decide(approvalId: string, allowed: boolean): Promise<Snapshot> { return isDesktop ? invoke('decide_approval', { approvalId, allowed }) : preview.decide(approvalId, allowed) },
  async revoke(agentId: string, key: PermissionKey): Promise<Snapshot> { return isDesktop ? invoke('revoke_permission', { agentId, key }) : preview.revoke(agentId, key) },
}

export function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message
  if (typeof error === 'object' && error && 'message' in error && typeof error.message === 'string') return error.message
  return typeof error === 'string' ? error : '操作失败，请重试。'
}
