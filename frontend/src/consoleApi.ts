import { invoke, isTauri } from '@tauri-apps/api/core'
import { deniedPermissions, permissionDescriptions, permissionLabels } from './types'
import type { Agent, AgentAction, CreateAgentInput, PermissionKey, Snapshot } from './types'

export const isDesktop = isTauri()

const previewAgents: Agent[] = [
  {
    id: 'agent-frontend', name: 'Frontend Architect', provider: 'Claude Code', version: '1.0.84',
    status: 'running', workspace: 'D:/workspaces/step-one-ui', securityMode: 'Protected',
    permissions: { workspaceWrite: true, network: false, mcpTools: false, clipboard: false, exportFiles: false },
    lastActivity: '现在', accent: 'violet',
  },
  {
    id: 'agent-research', name: 'Research Runner', provider: 'Codex', version: '0.42.0',
    status: 'ready', workspace: 'D:/workspaces/research', securityMode: 'Protected',
    permissions: deniedPermissions(), lastActivity: '12 分钟前', accent: 'blue',
  },
  {
    id: 'agent-local', name: 'Local Builder', provider: 'DeepSeek Harness', version: '0.8.2',
    status: 'stopped', workspace: 'D:/workspaces/local-builder', securityMode: 'Developer',
    permissions: { workspaceWrite: true, network: false, mcpTools: false, clipboard: false, exportFiles: false },
    lastActivity: '昨天', accent: 'orange',
  },
]

let preview: Snapshot = {
  agents: previewAgents,
  events: [
    { id: 'event-1', time: '10:42:18', title: '启动模拟会话', detail: 'Frontend Architect · Protected mode' },
    { id: 'event-2', time: '10:41:52', title: '挂载工作区', detail: 'step-one-ui · read/write' },
    { id: 'event-3', time: '10:39:04', title: '阻止网络请求', detail: 'connectivity disabled by policy' },
  ],
  approvals: [],
}

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

export const consoleApi = {
  async load(): Promise<Snapshot> {
    return isDesktop ? invoke('console_snapshot') : snapshot()
  },
  async chooseWorkspace(): Promise<string | null> {
    if (!isDesktop) return null
    return invoke('choose_workspace')
  },
  async create(input: CreateAgentInput): Promise<Snapshot> {
    if (isDesktop) return invoke('create_agent', { input })
    if (!input.name.trim() || input.name.trim().length > 80) throw new Error('请输入 1–80 个字符的名称。')
    if (!input.workspace.trim()) throw new Error('请输入模拟工作区名称。')
    const accent = input.provider === 'Claude Code' ? 'violet' : input.provider === 'Codex' ? 'blue' : 'orange'
    const version = input.provider === 'Claude Code' ? '1.0.84' : input.provider === 'Codex' ? '0.42.0' : '0.8.2'
    const agent: Agent = {
      ...input, name: input.name.trim(), id: crypto.randomUUID(), status: 'ready', permissions: deniedPermissions(),
      version, lastActivity: '刚刚', accent,
    }
    preview.agents.unshift(agent)
    record('创建模拟 Agent', agent.name)
    return snapshot()
  },
  async action(agentId: string, action: AgentAction): Promise<Snapshot> {
    if (isDesktop) return invoke('agent_action', { agentId, action })
    const agent = getAgent(agentId)
    if (preview.approvals.some(item => item.agentId === agentId)) throw new Error('请先处理待确认请求。')
    if (action === 'start' && (agent.status === 'ready' || agent.status === 'stopped')) agent.status = 'running'
    else if (action === 'stop' && agent.status === 'running') agent.status = 'stopped'
    else throw new Error('当前状态不支持此操作。')
    agent.lastActivity = '刚刚'
    record(action === 'start' ? '启动模拟会话' : '停止模拟会话', agent.name)
    return snapshot()
  },
  async request(agentId: string, action: 'reset' | 'delete' | PermissionKey): Promise<Snapshot> {
    if (isDesktop) return invoke('request_approval', { agentId, action })
    ensureEditable(agentId)
    const agent = getAgent(agentId)
    const title = action === 'reset' ? '重置模拟环境' : action === 'delete' ? '删除模拟 Agent' : permissionLabels[action]
    const detail = action === 'reset' ? '撤销所有模拟权限，保留实际工作区文件。'
      : action === 'delete' ? '移除模拟 Agent 配置，保留审计记录和实际工作区文件。'
        : `${permissionDescriptions[action]} 当前仅修改模拟策略。`
    preview.approvals.push({ id: crypto.randomUUID(), agentId, action, title, detail: `${agent.name}：${detail}` })
    record('请求用户确认', `${agent.name} · ${title}`)
    return snapshot()
  },
  async decide(approvalId: string, allowed: boolean): Promise<Snapshot> {
    if (isDesktop) return invoke('decide_approval', { approvalId, allowed })
    const approval = preview.approvals.find(item => item.id === approvalId)
    if (!approval) throw new Error('请求已处理或不存在。')
    const agent = getAgent(approval.agentId)
    if (agent.status === 'running') throw new Error('请先停止会话。')
    if (allowed) {
      if (approval.action === 'delete') preview.agents = preview.agents.filter(item => item.id !== agent.id)
      else if (approval.action === 'reset') { agent.status = 'ready'; agent.permissions = deniedPermissions() }
      else agent.permissions[approval.action as PermissionKey] = true
    }
    preview.approvals = preview.approvals.filter(item => item.id !== approvalId)
    record(allowed ? '用户确认' : '用户拒绝', approval.detail)
    return snapshot()
  },
  async revoke(agentId: string, key: PermissionKey): Promise<Snapshot> {
    if (isDesktop) return invoke('revoke_permission', { agentId, key })
    ensureEditable(agentId)
    const agent = getAgent(agentId)
    agent.permissions[key] = false
    record('撤销模拟权限', `${agent.name} · ${permissionLabels[key]}`)
    return snapshot()
  },
}

export function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message
  if (typeof error === 'object' && error && 'message' in error && typeof error.message === 'string') return error.message
  return typeof error === 'string' ? error : '操作失败，请重试。'
}
