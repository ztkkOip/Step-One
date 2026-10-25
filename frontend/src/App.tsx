import { useEffect, useMemo, useState } from 'react'
import type { ReactNode } from 'react'
import {
  Activity, ArrowUpRight, Bot, Check, ChevronRight, CircleHelp, Cloud, Command, FileKey2,
  FolderLock, Gauge, KeyRound, LayoutDashboard, LockKeyhole, MessageSquareText, MoreHorizontal,
  Network, Plus, RotateCcw, Server, Settings2, ShieldCheck, Sparkles, TerminalSquare, Trash2,
  WifiOff, X, Zap,
} from 'lucide-react'
import { consoleApi, errorMessage, isDesktop } from './consoleApi'
import { permissionDescriptions, permissionLabels } from './types'
import type { Agent, AgentAction, Approval, AuditEvent, CreateAgentInput, PermissionKey, Provider, Snapshot } from './types'

type ViewKey = 'overview' | 'agents' | 'connections' | 'audit'

const permissionKeys: PermissionKey[] = ['workspaceWrite', 'network', 'mcpTools', 'clipboard', 'exportFiles']
const providers: Array<{ name: Provider; description: string; version: string; icon: string; accent: string }> = [
  { name: 'Claude Code', description: 'Anthropic coding agent', version: '1.0.84', icon: 'CC', accent: 'violet' },
  { name: 'Codex', description: 'OpenAI coding agent', version: '0.42.0', icon: 'CX', accent: 'blue' },
  { name: 'DeepSeek Harness', description: 'DeepSeek agent runner', version: '0.8.2', icon: 'DS', accent: 'orange' },
]

const navItems: Array<{ key: ViewKey; label: string; icon: typeof LayoutDashboard }> = [
  { key: 'overview', label: 'Overview', icon: LayoutDashboard },
  { key: 'agents', label: 'Agents', icon: Bot },
  { key: 'connections', label: 'MCP & Connections', icon: Network },
  { key: 'audit', label: 'Audit log', icon: Activity },
]

function statusLabel(status: Agent['status']) {
  return { running: 'Running', ready: 'Ready', stopped: 'Stopped' }[status]
}

function App() {
  const [activeView, setActiveView] = useState<ViewKey>('overview')
  const [snapshot, setSnapshot] = useState<Snapshot>({ agents: [], events: [], approvals: [] })
  const [activeAgentId, setActiveAgentId] = useState<string | null>(null)
  const [isLoading, setIsLoading] = useState(true)
  const [isBusy, setIsBusy] = useState(false)
  const [isCreateOpen, setIsCreateOpen] = useState(false)
  const [isApprovalOpen, setIsApprovalOpen] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [toast, setToast] = useState<string | null>(null)

  const agents = snapshot.agents
  const activeAgent = agents.find(agent => agent.id === activeAgentId) ?? agents[0]
  const runningCount = agents.filter(agent => agent.status === 'running').length
  const protectedCount = agents.filter(agent => agent.securityMode === 'Protected').length
  const pendingApproval = snapshot.approvals[0]

  const applySnapshot = (next: Snapshot) => {
    setSnapshot(next)
    setActiveAgentId(current => next.agents.some(agent => agent.id === current) ? current : next.agents[0]?.id ?? null)
  }

  useEffect(() => {
    void consoleApi.load().then(next => {
      applySnapshot(next)
      setIsLoading(false)
    }).catch(reason => {
      setError(errorMessage(reason))
      setIsLoading(false)
    })
  }, [])

  useEffect(() => {
    if (!toast) return
    const timer = window.setTimeout(() => setToast(null), 3200)
    return () => window.clearTimeout(timer)
  }, [toast])

  const pageTitle = useMemo(() => navItems.find(item => item.key === activeView)?.label ?? 'Overview', [activeView])

  async function runOperation(operation: () => Promise<Snapshot>, success?: string): Promise<boolean> {
    setIsBusy(true)
    setError(null)
    try {
      applySnapshot(await operation())
      if (success) setToast(success)
      return true
    } catch (reason) {
      setError(errorMessage(reason))
      return false
    } finally {
      setIsBusy(false)
    }
  }

  async function requestApproval(action: 'reset' | 'delete' | PermissionKey) {
    if (!activeAgent) return
    if (await runOperation(() => consoleApi.request(activeAgent.id, action))) setIsApprovalOpen(true)
  }

  function handleAction(action: AgentAction) {
    if (!activeAgent) return
    void runOperation(
      () => consoleApi.action(activeAgent.id, action),
      `${activeAgent.name} 已${action === 'start' ? '启动' : '停止'}`,
    )
  }

  async function handleCreate(input: CreateAgentInput) {
    if (await runOperation(() => consoleApi.create(input), `${input.name.trim()} 已准备完成`)) setIsCreateOpen(false)
  }

  async function handleDecision(approval: Approval, allowed: boolean) {
    if (await runOperation(
      () => consoleApi.decide(approval.id, allowed),
      allowed ? '请求已确认' : '请求已拒绝',
    )) setIsApprovalOpen(false)
  }

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand-lockup">
          <div className="brand-mark"><ShieldCheck size={19} strokeWidth={2.5} /></div>
          <div><div className="brand-name">step-one</div><div className="brand-caption">secure agent workspace</div></div>
        </div>
        <div className="workspace-switcher"><span className="workspace-avatar">Z</span><div className="workspace-copy"><span className="workspace-label">Workspace</span><strong>Personal lab</strong></div><ChevronRight size={15} className="muted-icon" /></div>
        <div className="nav-section-label">Workspace</div>
        <nav className="main-nav" aria-label="Main navigation">
          {navItems.map(({ key, label, icon: Icon }) => <button key={key} className={`nav-item ${activeView === key ? 'active' : ''}`} onClick={() => setActiveView(key)}><Icon size={17} /><span>{label}</span>{key === 'agents' && <span className="nav-count">{agents.length}</span>}</button>)}
        </nav>
        <div className="nav-section-label">Manage</div>
        <nav className="main-nav"><button className="nav-item" onClick={() => setToast('设置中心将在凭据代理 PR 中启用')}><Settings2 size={17} /><span>Settings</span></button><button className="nav-item" onClick={() => setToast('帮助中心正在准备中')}><CircleHelp size={17} /><span>Help center</span></button></nav>
        <div className="sidebar-bottom"><div className="host-status"><span className="status-dot green" /><div><strong>Host protected</strong><span>Windows 11 · {isDesktop ? 'desktop' : 'simulator'}</span></div><MoreHorizontal size={16} className="muted-icon" /></div><div className="user-row"><div className="user-avatar">ZT</div><div className="workspace-copy"><strong>ztkkOip</strong><span>Local account</span></div><ChevronRight size={15} className="muted-icon" /></div></div>
      </aside>

      <main className="main-content">
        <header className="topbar"><div className="breadcrumb"><span>Personal lab</span><ChevronRight size={13} /><strong>{pageTitle}</strong></div><div className="topbar-actions"><div className="runtime-chip"><span className="pulse-dot" />{isDesktop ? 'Desktop control plane' : 'Simulator runtime'}</div><button className="icon-button" onClick={() => setToast('当前已是最新安全状态')} aria-label="Notifications"><Activity size={17} /></button><button className="avatar-button">ZT</button></div></header>
        <div className="content-wrap">
          <section className="welcome-row"><div><div className="section-kicker"><Sparkles size={13} /> Security workspace</div><h1>{activeView === 'overview' ? 'Good morning, ZT.' : pageTitle}</h1><p>{activeView === 'overview' ? 'Your agents are ready. Everything outside their permissions stays outside.' : 'Manage your local agent environment with clear, human-readable controls.'}</p></div><button className="primary-button" onClick={() => setIsCreateOpen(true)}><Plus size={17} /> New agent</button></section>
          {error && <div className="error-banner"><MessageSquareText size={15} /><span>{error}</span><button onClick={() => setError(null)} aria-label="Dismiss error"><X size={14} /></button></div>}
          {pendingApproval && <div className="approval-banner"><ShieldCheck size={16} /><div><strong>需要你的确认</strong><span>{pendingApproval.title} · {pendingApproval.detail}</span></div><button className="secondary-button" onClick={() => setIsApprovalOpen(true)}>Review</button></div>}
          {activeView === 'overview' && <><section className="hero-card"><div className="hero-copy"><div className="hero-badge"><LockKeyhole size={14} /> Protected by default</div><h2>Build with confidence.<br /><em>Stay in control.</em></h2><p>Every agent runs in its own workspace. Your credentials stay on the host, and permissions are visible before anything starts.</p><button className="secondary-button" onClick={() => setActiveView('connections')}>Review security model <ArrowUpRight size={15} /></button></div><div className="hero-visual" aria-hidden="true"><div className="orbit orbit-one" /><div className="orbit orbit-two" /><div className="core-shield"><ShieldCheck size={42} strokeWidth={1.45} /></div><div className="orbit-node node-one"><KeyRound size={15} /></div><div className="orbit-node node-two"><FolderLock size={15} /></div><div className="orbit-node node-three"><WifiOff size={15} /></div></div></section><section className="stats-grid"><StatCard label="Active agents" value={String(runningCount)} detail={`of ${agents.length} workspaces`} icon={<Bot size={18} />} tone="violet" /><StatCard label="Protected workspaces" value={String(protectedCount)} detail="credentials isolated" icon={<LockKeyhole size={18} />} tone="green" /><StatCard label="Pending approvals" value={String(snapshot.approvals.length)} detail="user decisions" icon={<ShieldCheck size={18} />} tone="orange" /><StatCard label="Host health" value="Good" detail="all systems normal" icon={<Gauge size={18} />} tone="blue" /></section></>}
          {isLoading ? <LoadingState /> : activeView === 'connections' ? <ConnectionsView onToast={setToast} /> : activeView === 'audit' ? <AuditView events={snapshot.events} /> : <AgentWorkspace agents={agents} events={snapshot.events} activeAgent={activeAgent} activeView={activeView} activeAgentId={activeAgentId} isBusy={isBusy} onSelect={setActiveAgentId} onCreate={() => setIsCreateOpen(true)} onAction={handleAction} onRequest={requestApproval} onRevoke={(key) => activeAgent && void runOperation(() => consoleApi.revoke(activeAgent.id, key), `${permissionLabels[key]} 已撤销`)} />}
        </div>
      </main>
      {isCreateOpen && <CreateAgentModal isBusy={isBusy} onClose={() => setIsCreateOpen(false)} onCreate={handleCreate} />}
      {isApprovalOpen && pendingApproval && <ApprovalModal approval={pendingApproval} isBusy={isBusy} onClose={() => setIsApprovalOpen(false)} onDecision={handleDecision} />}
      {toast && <div className="toast"><span className="toast-check"><Check size={14} /></span>{toast}<button onClick={() => setToast(null)} aria-label="Dismiss"><X size={14} /></button></div>}
    </div>
  )
}

function AgentWorkspace({ agents, events, activeAgent, activeView, activeAgentId, isBusy, onSelect, onCreate, onAction, onRequest, onRevoke }: { agents: Agent[]; events: AuditEvent[]; activeAgent?: Agent; activeView: ViewKey; activeAgentId: string | null; isBusy: boolean; onSelect: (id: string) => void; onCreate: () => void; onAction: (action: AgentAction) => void; onRequest: (action: 'reset' | 'delete' | PermissionKey) => void; onRevoke: (key: PermissionKey) => void }) {
  return <><section className="section-header"><div><h2>{activeView === 'agents' ? 'All agents' : 'Your agents'}</h2><p>{activeView === 'agents' ? 'Each workspace has its own security boundary.' : 'Pick up where you left off.'}</p></div><button className="quiet-button" onClick={onCreate}><Plus size={15} /> Add agent</button></section><section className="agents-grid">{agents.map(agent => <AgentCard key={agent.id} agent={agent} selected={agent.id === activeAgentId} onSelect={() => onSelect(agent.id)} />)}<button className="add-agent-card" onClick={onCreate}><span><Plus size={20} /></span><strong>Create an agent</strong><small>Start from a protected template</small></button></section><section className="lower-grid"><div className="panel activity-panel"><div className="panel-heading"><div><h2>Recent activity</h2><p>Last actions across your workspace.</p></div></div><div className="activity-list">{events.slice(0, 4).map(event => <ActivityRow key={event.id} event={event} />)}</div></div><SelectedAgent agent={activeAgent} isBusy={isBusy} onAction={onAction} onRequest={onRequest} onRevoke={onRevoke} /></section></>
}

function SelectedAgent({ agent, isBusy, onAction, onRequest, onRevoke }: { agent?: Agent; isBusy: boolean; onAction: (action: AgentAction) => void; onRequest: (action: 'reset' | 'delete' | PermissionKey) => void; onRevoke: (key: PermissionKey) => void }) {
  return <div className="panel selected-panel"><div className="panel-heading"><div><h2>Selected agent</h2><p>Quick controls for this workspace.</p></div>{agent && <span className={`status-pill ${agent.status}`}>{statusLabel(agent.status)}</span>}</div>{agent ? <><div className="selected-agent"><div className={`agent-logo ${agent.accent}`}><Bot size={21} /></div><div><strong>{agent.name}</strong><span>{agent.provider} · v{agent.version}</span></div></div><div className="permission-list">{permissionKeys.map(key => <PermissionControl key={key} agent={agent} permission={key} onRequest={onRequest} onRevoke={onRevoke} />)}</div><div className="control-row"><button className="control-button primary" disabled={isBusy || agent.status === 'running'} onClick={() => onAction('start')}><Zap size={15} /> Start</button><button className="control-button" disabled={isBusy || agent.status !== 'running'} onClick={() => onAction('stop')}><TerminalSquare size={15} /> Stop</button><button className="control-button" disabled={isBusy || agent.status === 'running'} onClick={() => onRequest('reset')}><RotateCcw size={15} /> Reset</button><button className="control-button danger icon-only" disabled={isBusy || agent.status === 'running'} onClick={() => onRequest('delete')} aria-label="Delete agent"><Trash2 size={15} /></button></div></> : <div className="empty-selection">Create an agent to view controls.</div>}</div>
}

function PermissionControl({ agent, permission, onRequest, onRevoke }: { agent: Agent; permission: PermissionKey; onRequest: (action: PermissionKey) => void; onRevoke: (key: PermissionKey) => void }) {
  const granted = agent.permissions[permission]
  return <div className="permission-control"><div className="permission-item"><span className="permission-icon"><PermissionIcon permission={permission} /></span><div><span>{permissionLabels[permission]}</span><strong>{granted ? 'Allowed' : 'Blocked by default'}</strong></div><span className={`permission-check ${granted ? '' : 'muted'}`}>{granted ? <Check size={12} /> : <WifiOff size={12} />}</span></div>{granted ? <button className="permission-action" onClick={() => onRevoke(permission)}>Revoke</button> : <button className="permission-action" onClick={() => onRequest(permission)}>Request</button>}</div>
}

function PermissionIcon({ permission }: { permission: PermissionKey }) { return permission === 'workspaceWrite' ? <FolderLock size={15} /> : permission === 'network' ? <WifiOff size={15} /> : permission === 'mcpTools' ? <Network size={15} /> : permission === 'clipboard' ? <MessageSquareText size={15} /> : <FileKey2 size={15} /> }

function StatCard({ label, value, detail, icon, tone }: { label: string; value: string; detail: string; icon: ReactNode; tone: string }) { return <div className="stat-card"><div className={`stat-icon ${tone}`}>{icon}</div><div className="stat-copy"><span>{label}</span><strong>{value}</strong><small>{detail}</small></div><ArrowUpRight size={15} className="stat-arrow" /></div> }
function AgentCard({ agent, selected, onSelect }: { agent: Agent; selected: boolean; onSelect: () => void }) { return <button className={`agent-card ${selected ? 'selected' : ''}`} onClick={onSelect}><div className="agent-card-top"><div className={`agent-logo ${agent.accent}`}><Bot size={22} /></div><span className={`status-pill ${agent.status}`}><span className="status-dot" />{statusLabel(agent.status)}</span></div><div className="agent-card-copy"><strong>{agent.name}</strong><span>{agent.provider} · v{agent.version}</span></div><div className="agent-card-footer"><span><FolderLock size={13} /> {agent.securityMode} mode</span><ChevronRight size={15} /></div></button> }
function AuditView({ events }: { events: AuditEvent[] }) { return <section className="panel full-panel"><div className="panel-heading"><div><h2>Audit log</h2><p>Every sandbox decision is recorded on the host.</p></div><div className="secure-label"><ShieldCheck size={14} /> Local only</div></div><div className="activity-list audit-list">{events.length ? events.map(event => <ActivityRow key={event.id} event={event} />) : <div className="activity-empty">No audit events yet.</div>}</div></section> }
function ActivityRow({ event }: { event: AuditEvent }) { return <div className="activity-row"><span className="activity-dot green" /><div className="activity-copy"><strong>{event.title}</strong><span>{event.detail}</span></div><time>{event.time}</time></div> }
function ConnectionsView({ onToast }: { onToast: (message: string) => void }) { const connections = [{ name: 'Model proxy', description: 'Credential boundary is reserved for the host broker', icon: Cloud, state: 'Simulator', tone: 'blue' }, { name: 'GitHub', description: 'OAuth connection for repository tools', icon: Command, state: 'Not connected', tone: 'muted' }, { name: 'MCP Gateway', description: 'Tool permissions stay inside policy', icon: Server, state: 'Simulator', tone: 'blue' }]; return <section className="panel full-panel connections-panel"><div className="panel-heading"><div><h2>MCP & Connections</h2><p>Connect capabilities without placing secrets inside an agent.</p></div><div className="secure-label"><LockKeyhole size={14} /> Secrets stay on host</div></div><div className="connection-list">{connections.map(({ name, description, icon: Icon, state, tone }) => <div className="connection-row" key={name}><div className="connection-icon"><Icon size={19} /></div><div className="connection-copy"><strong>{name}</strong><span>{description}</span></div><span className={`connection-state ${tone}`}>{state}</span><button className="quiet-button" onClick={() => onToast(`${name} 的配置将在后续安全连接 PR 中启用`)}>{state === 'Not connected' ? 'Connect' : 'Manage'} <ChevronRight size={14} /></button></div>)}</div></section> }

function CreateAgentModal({ isBusy, onClose, onCreate }: { isBusy: boolean; onClose: () => void; onCreate: (input: CreateAgentInput) => Promise<void> }) {
  const [name, setName] = useState('')
  const [provider, setProvider] = useState<Provider>('Claude Code')
  const [workspace, setWorkspace] = useState('')
  const [securityMode, setSecurityMode] = useState<'Protected' | 'Developer'>('Protected')
  const [workspaceError, setWorkspaceError] = useState<string | null>(null)
  async function chooseWorkspace() { const selected = await consoleApi.chooseWorkspace(); if (selected) setWorkspace(selected) }
  async function submit(event: React.FormEvent) { event.preventDefault(); if (!name.trim() || !workspace.trim()) { setWorkspaceError('名称和工作区不能为空。'); return }; setWorkspaceError(null); await onCreate({ name, provider, workspace, securityMode }) }
  return <div className="modal-backdrop" onMouseDown={onClose}><form className="modal-card form-modal" onSubmit={(event) => void submit(event)} onMouseDown={event => event.stopPropagation()}><div className="modal-header"><div><div className="section-kicker"><Sparkles size={13} /> New workspace</div><h2>Configure your agent</h2><p>Protected mode starts with no permissions.</p></div><button type="button" className="icon-button" onClick={onClose} aria-label="Close"><X size={18} /></button></div><label className="field-label">Agent name<input value={name} onChange={event => setName(event.target.value)} placeholder="e.g. Frontend Architect" maxLength={80} /></label><label className="field-label">Provider<select value={provider} onChange={event => setProvider(event.target.value as Provider)}>{providers.map(item => <option key={item.name}>{item.name}</option>)}</select></label><label className="field-label">Workspace<div className="workspace-field"><input value={workspace} onChange={event => setWorkspace(event.target.value)} placeholder={isDesktop ? 'Choose a local folder' : 'e.g. D:/workspaces/my-agent'} /><button type="button" className="quiet-button" onClick={() => void chooseWorkspace()}>Browse</button></div></label><label className="field-label">Security mode<select value={securityMode} onChange={event => setSecurityMode(event.target.value as 'Protected' | 'Developer')}><option value="Protected">Protected</option><option value="Developer">Developer</option></select></label>{workspaceError && <div className="field-error">{workspaceError}</div>}<div className="modal-actions"><button type="button" className="quiet-button" onClick={onClose}>Cancel</button><button type="submit" className="primary-button" disabled={isBusy}>{isBusy ? 'Creating…' : 'Create agent'}</button></div><div className="modal-note"><LockKeyhole size={14} /><span>API keys and credentials remain on your host. The selected folder is the only workspace boundary.</span></div></form></div>
}

function ApprovalModal({ approval, isBusy, onClose, onDecision }: { approval: Approval; isBusy: boolean; onClose: () => void; onDecision: (approval: Approval, allowed: boolean) => void }) { return <div className="modal-backdrop" onMouseDown={onClose}><div className="modal-card" onMouseDown={event => event.stopPropagation()}><div className="modal-header"><div><div className="section-kicker"><ShieldCheck size={13} /> Permission review</div><h2>{approval.title}</h2><p>Step-One will record your decision locally.</p></div><button className="icon-button" onClick={onClose} aria-label="Close"><X size={18} /></button></div><div className="approval-detail"><strong>{approval.detail.split('：')[0]}</strong><span>{approval.detail.split('：').slice(1).join('：')}</span></div><div className="modal-actions"><button className="quiet-button" disabled={isBusy} onClick={() => onDecision(approval, false)}>Deny</button><button className="primary-button" disabled={isBusy} onClick={() => onDecision(approval, true)}>Allow</button></div><div className="modal-note"><LockKeyhole size={14} /><span>Approval changes the policy for this Agent. It does not expose host credentials or grant arbitrary host access.</span></div></div></div> }
function LoadingState() { return <section className="panel loading-state"><div className="loading-bar" /><div className="loading-bar short" /><span>Loading security workspace…</span></section> }

export default App
