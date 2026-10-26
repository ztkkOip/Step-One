import { useEffect, useMemo, useRef, useState } from 'react'
import type { ReactNode } from 'react'
import {
  Activity, ArrowUpRight, Bot, Check, ChevronRight, CircleHelp, Cloud, Command, FileKey2,
  FolderLock, Gauge, KeyRound, LayoutDashboard, LockKeyhole, MessageSquareText, MoreHorizontal,
  Network, Plus, RotateCcw, Server, Settings2, ShieldCheck, Sparkles, TerminalSquare, Trash2,
  WifiOff, X, Zap,
} from 'lucide-react'
import { consoleApi, errorMessage, isDesktop } from './consoleApi'
import { agentPending, deniedPermissions, permissionLabels } from './types'
import type { Agent, AgentAction, Approval, AuditEvent, CreateAgentInput, PermissionKey, Provider, Snapshot, RuntimeInfo, WorkspaceGrant, RuntimeCapabilities } from './types'

type ViewKey = 'overview' | 'agents' | 'connections' | 'audit'

const permissionKeys: PermissionKey[] = ['workspaceWrite', 'network', 'mcpTools', 'clipboard', 'exportFiles']
const providers: Array<{ name: Provider; description: string; version: string; icon: string; accent: string }> = [
  { name: 'Claude Code', description: 'Anthropic coding agent', version: 'not-installed', icon: 'CC', accent: 'violet' },
  { name: 'Codex', description: 'OpenAI coding agent', version: 'not-installed', icon: 'CX', accent: 'blue' },
  { name: 'DeepSeek Harness', description: 'DeepSeek agent runner', version: 'not-installed', icon: 'DS', accent: 'orange' },
]

const navItems: Array<{ key: ViewKey; label: string; icon: typeof LayoutDashboard }> = [
  { key: 'overview', label: 'Overview', icon: LayoutDashboard },
  { key: 'agents', label: 'Agents', icon: Bot },
  { key: 'connections', label: 'MCP & Connections', icon: Network },
  { key: 'audit', label: 'Audit log', icon: Activity },
]

function statusLabel(status: Agent['status']) {
  return { running: 'Running', ready: 'Ready', stopped: 'Stopped', unknown: 'Unknown' }[status]
}

function App() {
  const [activeView, setActiveView] = useState<ViewKey>('overview')
  const [snapshot, setSnapshot] = useState<Snapshot>({ agents: [], events: [], approvals: [], operations: [], runtime: { kind: 'simulator', available: false, isolationEnforced: false, capabilities: deniedPermissions(), message: '正在读取运行环境…' } })
  const [diagnostics, setDiagnostics] = useState<RuntimeCapabilities | null>(null)
  const [isCheckingRuntime, setIsCheckingRuntime] = useState(true)
  const [runtimeError, setRuntimeError] = useState<string | null>(null)
  const [activeAgentId, setActiveAgentId] = useState<string | null>(null)
  const [isLoading, setIsLoading] = useState(true)
  const [isBusy, setIsBusy] = useState(false)
  const [isCreateOpen, setIsCreateOpen] = useState(false)
  const [isApprovalOpen, setIsApprovalOpen] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [toast, setToast] = useState<string | null>(null)

  const appliedGeneration = useRef(0)
  const agents = snapshot.agents
  const activeAgent = agents.find(agent => agent.id === activeAgentId) ?? agents[0]
  const runningCount = agents.filter(agent => agent.status === 'running').length
  const protectedCount = agents.filter(agent => agent.securityMode === 'Protected').length
  const pendingApproval = snapshot.approvals[0]
  const pendingOperation = snapshot.operations.some(operation => operation.state === 'pending')

  const applySnapshot = (next: Snapshot) => {
    appliedGeneration.current += 1
    setSnapshot(next)
    setActiveAgentId(current => next.agents.some(agent => agent.id === current) ? current : next.agents[0]?.id ?? null)
  }

  async function checkRuntime() {
    setIsCheckingRuntime(true)
    setRuntimeError(null)
    setDiagnostics(null)
    try { setDiagnostics(await consoleApi.runtimeCapabilities()) }
    catch (reason) { setRuntimeError(errorMessage(reason)) }
    finally { setIsCheckingRuntime(false) }
  }

  useEffect(() => {
    void checkRuntime()
    const generation = appliedGeneration.current
    void consoleApi.load().then(next => {
      if (generation === appliedGeneration.current) applySnapshot(next)
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

  useEffect(() => {
    if (!pendingOperation && !isBusy) return
    let cancelled = false
    let timer: ReturnType<typeof setTimeout>
    async function refresh() {
      try {
        const generation = appliedGeneration.current
        const next = await consoleApi.load()
        if (!cancelled && generation === appliedGeneration.current) {
          applySnapshot(next)
          if (isBusy || next.operations.some(operation => operation.state === 'pending')) timer = setTimeout(() => void refresh(), 1000)
        }
      } catch (reason) {
        if (!cancelled) setError(errorMessage(reason))
      }
    }
    timer = setTimeout(() => void refresh(), 1000)
    return () => { cancelled = true; clearTimeout(timer) }
  }, [pendingOperation, isBusy])

  const pageTitle = useMemo(() => navItems.find(item => item.key === activeView)?.label ?? 'Overview', [activeView])

  async function runOperation(operation: () => Promise<Snapshot>, success?: string): Promise<boolean> {
    appliedGeneration.current += 1
    setIsBusy(true)
    setError(null)
    try {
      const next = await operation()
      applySnapshot(next)
      if (next.operations.some(item => item.state === 'pending')) setToast('操作已提交，正在等待运行环境确认。')
      else if (success) setToast(success)
      return true
    } catch (reason) {
      setError(errorMessage(reason))
      try { applySnapshot(await consoleApi.load()) } catch { /* Keep the last confirmed state if refresh also fails. */ }
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
        <div className="sidebar-bottom"><div className="host-status"><span className="status-dot green" /><div><strong>{snapshot.runtime.isolationEnforced ? 'Isolation active' : 'Simulation only'}</strong><span>Windows 11 · {isDesktop ? 'desktop' : 'simulator'}</span></div><MoreHorizontal size={16} className="muted-icon" /></div><div className="user-row"><div className="user-avatar">ZT</div><div className="workspace-copy"><strong>ztkkOip</strong><span>Local account</span></div><ChevronRight size={15} className="muted-icon" /></div></div>
      </aside>

      <main className="main-content">
        <header className="topbar"><div className="breadcrumb"><span>Personal lab</span><ChevronRight size={13} /><strong>{pageTitle}</strong></div><div className="topbar-actions"><div className="runtime-chip"><span className="pulse-dot" />{snapshot.runtime.kind === 'hyperv' ? 'Hyper-V runtime' : 'Simulator runtime'}</div><button className="icon-button" onClick={() => setToast('当前已是最新安全状态')} aria-label="Notifications"><Activity size={17} /></button><button className="avatar-button">ZT</button></div></header>
        <div className="content-wrap">
          <section className="welcome-row"><div><div className="section-kicker"><Sparkles size={13} /> Security workspace</div><h1>{activeView === 'overview' ? 'Good morning, ZT.' : pageTitle}</h1><p>{activeView === 'overview' ? 'Manage agents and review permissions before starting a session.' : 'Manage your local agent environment with clear, human-readable controls.'}</p></div><button className="primary-button" onClick={() => setIsCreateOpen(true)}><Plus size={17} /> New agent</button></section>
          {error && <div className="error-banner" role="alert"><MessageSquareText size={15} /><span>{error}</span><button onClick={() => setError(null)} aria-label="Dismiss error"><X size={14} /></button></div>}
          <div className="approval-banner" role="status"><ShieldCheck size={16} /><div><strong>{snapshot.runtime.isolationEnforced ? '隔离已启用' : '当前没有真实隔离'}</strong><span>{snapshot.runtime.message}</span></div><button className="quiet-button" disabled={isBusy} onClick={() => void runOperation(() => consoleApi.load())}>Refresh</button></div>
          {pendingApproval && <div className="approval-banner"><ShieldCheck size={16} /><div><strong>需要你的确认</strong><span>{pendingApproval.title} · {pendingApproval.detail}</span></div><button className="secondary-button" onClick={() => setIsApprovalOpen(true)}>Review</button></div>}
          {activeView === 'overview' && <><section className="hero-card"><div className="hero-copy"><div className="hero-badge"><LockKeyhole size={14} /> Permissions denied by default</div><h2>Build with confidence.<br /><em>Stay in control.</em></h2><p>Review the selected workspace and permissions. Simulator mode models decisions without running agents or accessing workspace files.</p><button className="secondary-button" onClick={() => setActiveView('connections')}>Review security model <ArrowUpRight size={15} /></button></div><div className="hero-visual" aria-hidden="true"><div className="orbit orbit-one" /><div className="orbit orbit-two" /><div className="core-shield"><ShieldCheck size={42} strokeWidth={1.45} /></div><div className="orbit-node node-one"><KeyRound size={15} /></div><div className="orbit-node node-two"><FolderLock size={15} /></div><div className="orbit-node node-three"><WifiOff size={15} /></div></div></section><section className="stats-grid"><StatCard label="Active agents" value={String(runningCount)} detail={`of ${agents.length} workspaces`} icon={<Bot size={18} />} tone="violet" /><StatCard label="Protected profiles" value={String(protectedCount)} detail="policy profiles" icon={<LockKeyhole size={18} />} tone="green" /><StatCard label="Pending approvals" value={String(snapshot.approvals.length)} detail="user decisions" icon={<ShieldCheck size={18} />} tone="orange" /><StatCard label="Runtime" value={snapshot.runtime.available ? 'Ready' : 'Unavailable'} detail={snapshot.runtime.isolationEnforced ? 'isolation active' : 'no isolation'} icon={<Gauge size={18} />} tone="blue" /></section></>}
          {isLoading ? <LoadingState /> : activeView === 'connections' ? <ConnectionsView diagnostics={diagnostics} isCheckingRuntime={isCheckingRuntime} runtimeError={runtimeError} onCheckRuntime={() => void checkRuntime()} onToast={setToast} /> : activeView === 'audit' ? <AuditView events={snapshot.events} /> : <AgentWorkspace agents={agents} events={snapshot.events} activeAgent={activeAgent} activeView={activeView} activeAgentId={activeAgentId} runtime={snapshot.runtime} isBusy={isBusy || !!activeAgent && agentPending(activeAgent)} onSelect={setActiveAgentId} onCreate={() => setIsCreateOpen(true)} onAction={handleAction} onRequest={requestApproval} onRevoke={(key) => activeAgent && void runOperation(() => consoleApi.revoke(activeAgent.id, key), `${permissionLabels[key]} 已撤销`)} />}
        </div>
      </main>
      {isCreateOpen && <CreateAgentModal runtime={snapshot.runtime} isBusy={isBusy} onClose={() => setIsCreateOpen(false)} onCreate={handleCreate} />}
      {isApprovalOpen && pendingApproval && <ApprovalModal approval={pendingApproval} isBusy={isBusy} onClose={() => setIsApprovalOpen(false)} onDecision={handleDecision} />}
      {toast && <div className="toast"><span className="toast-check"><Check size={14} /></span>{toast}<button onClick={() => setToast(null)} aria-label="Dismiss"><X size={14} /></button></div>}
    </div>
  )
}

function AgentWorkspace({ agents, events, activeAgent, activeView, activeAgentId, runtime, isBusy, onSelect, onCreate, onAction, onRequest, onRevoke }: { agents: Agent[]; events: AuditEvent[]; activeAgent?: Agent; activeView: ViewKey; activeAgentId: string | null; runtime: RuntimeInfo; isBusy: boolean; onSelect: (id: string) => void; onCreate: () => void; onAction: (action: AgentAction) => void; onRequest: (action: 'reset' | 'delete' | PermissionKey) => void; onRevoke: (key: PermissionKey) => void }) {
  return <><section className="section-header"><div><h2>{activeView === 'agents' ? 'All agents' : 'Your agents'}</h2><p>{activeView === 'agents' ? 'Each workspace has its own security boundary.' : 'Pick up where you left off.'}</p></div><button className="quiet-button" onClick={onCreate}><Plus size={15} /> Add agent</button></section><section className="agents-grid">{agents.map(agent => <AgentCard key={agent.id} agent={agent} selected={agent.id === activeAgentId} onSelect={() => onSelect(agent.id)} />)}<button className="add-agent-card" onClick={onCreate}><span><Plus size={20} /></span><strong>Create an agent</strong><small>Start from a protected template</small></button></section><section className="lower-grid"><div className="panel activity-panel"><div className="panel-heading"><div><h2>Recent activity</h2><p>Last actions across your workspace.</p></div></div><div className="activity-list">{events.slice(0, 4).map(event => <ActivityRow key={event.id} event={event} />)}</div></div><SelectedAgent agent={activeAgent} runtime={runtime} isBusy={isBusy} onAction={onAction} onRequest={onRequest} onRevoke={onRevoke} /></section></>
}

function SelectedAgent({ agent, runtime, isBusy, onAction, onRequest, onRevoke }: { agent?: Agent; runtime: RuntimeInfo; isBusy: boolean; onAction: (action: AgentAction) => void; onRequest: (action: 'reset' | 'delete' | PermissionKey) => void; onRevoke: (key: PermissionKey) => void }) {
  return <div className="panel selected-panel"><div className="panel-heading"><div><h2>Selected agent</h2><p>Quick controls for this workspace.</p></div>{agent && <span className={`status-pill ${agent.status}`}>{statusLabel(agent.status)}</span>}</div>{agent ? <><div className="selected-agent"><div className={`agent-logo ${agent.accent}`}><Bot size={21} /></div><div><strong>{agent.name}</strong><span>{agent.provider} · {agent.version === 'not-installed' ? '未安装' : agent.version}</span></div></div><p role="status">运行环境：{agent.runtimeState}{agentPending(agent) ? ' · 操作处理中' : ''}</p>{agent.lastError && <div role="alert">{agent.lastError.message}{agent.lastError.retryable && <span> · 可以重试</span>}</div>}<div className="permission-list">{permissionKeys.map(key => <PermissionControl key={key} agent={agent} permission={key} runtime={runtime} isBusy={isBusy} onRequest={onRequest} onRevoke={onRevoke} />)}</div><div className="control-row"><button className="control-button primary" disabled={isBusy || !runtime.available || (agent.status !== 'ready' && agent.status !== 'stopped') || agent.runtimeState === 'failed' || agent.runtimeState === 'missing'} onClick={() => onAction('start')}><Zap size={15} /> Start</button><button className="control-button" disabled={isBusy || (agent.status !== 'running' && agent.status !== 'unknown')} onClick={() => onAction('stop')}><TerminalSquare size={15} /> Stop</button><button className="control-button" disabled={isBusy || agent.status === 'running' || agent.status === 'unknown'} onClick={() => onRequest('reset')}><RotateCcw size={15} /> Reset</button><button className="control-button danger icon-only" disabled={isBusy || agent.status === 'running' || agent.status === 'unknown'} onClick={() => onRequest('delete')} aria-label="Delete agent"><Trash2 size={15} /></button></div></> : <div className="empty-selection">Create an agent to view controls.</div>}</div>
}

function PermissionControl({ agent, permission, runtime, isBusy, onRequest, onRevoke }: { agent: Agent; permission: PermissionKey; runtime: RuntimeInfo; isBusy: boolean; onRequest: (action: PermissionKey) => void; onRevoke: (key: PermissionKey) => void }) {
  const granted = agent.permissions[permission]
  const supported = runtime.capabilities[permission]
  const disabled = isBusy || agent.status === 'running' || agent.status === 'unknown' || !supported
  return <div className="permission-control"><div className="permission-item"><span className="permission-icon"><PermissionIcon permission={permission} /></span><div><span>{permissionLabels[permission]}</span><strong>{!supported ? 'Unavailable' : granted ? (runtime.isolationEnforced ? 'Allowed' : 'Simulated allowance') : 'Blocked by default'}</strong></div><span className={`permission-check ${granted ? '' : 'muted'}`}>{granted ? <Check size={12} /> : <WifiOff size={12} />}</span></div>{granted ? <button className="permission-action" disabled={disabled} onClick={() => onRevoke(permission)}>Revoke</button> : <button className="permission-action" disabled={disabled} onClick={() => onRequest(permission)}>Request</button>}</div>
}

function PermissionIcon({ permission }: { permission: PermissionKey }) { return permission === 'workspaceWrite' ? <FolderLock size={15} /> : permission === 'network' ? <WifiOff size={15} /> : permission === 'mcpTools' ? <Network size={15} /> : permission === 'clipboard' ? <MessageSquareText size={15} /> : <FileKey2 size={15} /> }

function StatCard({ label, value, detail, icon, tone }: { label: string; value: string; detail: string; icon: ReactNode; tone: string }) { return <div className="stat-card"><div className={`stat-icon ${tone}`}>{icon}</div><div className="stat-copy"><span>{label}</span><strong>{value}</strong><small>{detail}</small></div><ArrowUpRight size={15} className="stat-arrow" /></div> }
function AgentCard({ agent, selected, onSelect }: { agent: Agent; selected: boolean; onSelect: () => void }) { return <button className={`agent-card ${selected ? 'selected' : ''}`} onClick={onSelect}><div className="agent-card-top"><div className={`agent-logo ${agent.accent}`}><Bot size={22} /></div><span className={`status-pill ${agent.status}`}><span className="status-dot" />{statusLabel(agent.status)}</span></div><div className="agent-card-copy"><strong>{agent.name}</strong><span>{agent.provider} · {agent.version === 'not-installed' ? '未安装' : agent.version}</span></div><div className="agent-card-footer"><span><FolderLock size={13} /> {agent.securityMode} mode</span><ChevronRight size={15} /></div></button> }
function AuditView({ events }: { events: AuditEvent[] }) { return <section className="panel full-panel"><div className="panel-heading"><div><h2>Audit log</h2><p>Every sandbox decision is recorded on the host.</p></div><div className="secure-label"><ShieldCheck size={14} /> Local only</div></div><div className="activity-list audit-list">{events.length ? events.map(event => <ActivityRow key={event.id} event={event} />) : <div className="activity-empty">No audit events yet.</div>}</div></section> }
function ActivityRow({ event }: { event: AuditEvent }) { return <div className="activity-row"><span className="activity-dot green" /><div className="activity-copy"><strong>{event.title}</strong><span>{event.detail}</span></div><time>{event.time}</time></div> }
function ConnectionsView({ diagnostics, isCheckingRuntime, runtimeError, onCheckRuntime, onToast }: { diagnostics: RuntimeCapabilities | null; isCheckingRuntime: boolean; runtimeError: string | null; onCheckRuntime: () => void; onToast: (message: string) => void }) {
  const diagnosticsState = isCheckingRuntime ? 'Checking' : runtimeError ? 'Unknown' : diagnostics?.available ? 'Prerequisites passed' : 'Blocked'
  const connections = [
    { name: 'Runtime diagnostics', description: '只读前置条件检查，不会启用或切换运行时。', icon: Gauge, state: diagnosticsState, tone: diagnostics?.available && !isCheckingRuntime && !runtimeError ? 'green' : 'muted' },
    { name: 'Model proxy', description: 'Credential boundary is reserved for the host broker', icon: Cloud, state: 'Not connected', tone: 'muted' },
    { name: 'GitHub', description: 'OAuth connection for repository tools', icon: Command, state: 'Not connected', tone: 'muted' },
    { name: 'MCP Gateway', description: 'Tool permissions stay inside policy', icon: Server, state: 'Not connected', tone: 'muted' },
  ]
  return <section className="panel full-panel connections-panel"><div className="panel-heading"><div><h2>MCP & Connections</h2><p>Connect capabilities without placing secrets inside an agent.</p></div><div className="secure-label"><LockKeyhole size={14} /> Secrets stay on host</div></div>
    <div className="runtime-check-list" aria-live="polite">{isCheckingRuntime ? <div className="runtime-check"><span className="check-marker unknown" />正在检查 Windows 沙箱前置条件…</div> : runtimeError ? <div className="runtime-check" role="alert"><span className="check-marker blocked" />{runtimeError}</div> : diagnostics?.checks.map(item => <div className="runtime-check" key={item.id}><span className={`check-marker ${item.status}`} />{item.message}</div>)}</div>
    <div className="connection-list">{connections.map(({ name, description, icon: Icon, state, tone }) => <div className="connection-row" key={name}><div className="connection-icon"><Icon size={19} /></div><div className="connection-copy"><strong>{name}</strong><span>{description}</span></div><span className={`connection-state ${tone}`}>{state}</span><button className="quiet-button" onClick={() => name === 'Runtime diagnostics' ? onCheckRuntime() : onToast(`${name} 的配置将在后续安全连接 PR 中启用`)} disabled={name === 'Runtime diagnostics' && isCheckingRuntime}>{name === 'Runtime diagnostics' ? 'Recheck' : 'Connect'} <ChevronRight size={14} /></button></div>)}</div>
  </section>
}

function CreateAgentModal({ runtime, isBusy, onClose, onCreate }: { runtime: RuntimeInfo; isBusy: boolean; onClose: () => void; onCreate: (input: CreateAgentInput) => Promise<void> }) {
  const [name, setName] = useState('')
  const [provider, setProvider] = useState<Provider>('Claude Code')
  const [workspace, setWorkspace] = useState<WorkspaceGrant | null>(null)
  const [securityMode, setSecurityMode] = useState<'Protected' | 'Developer'>('Protected')
  const [workspaceError, setWorkspaceError] = useState<string | null>(null)
  const [choosing, setChoosing] = useState(false)
  async function chooseWorkspace() {
    setChoosing(true)
    setWorkspaceError(null)
    try { const selected = await consoleApi.chooseWorkspace(); if (selected) setWorkspace(selected) }
    catch (reason) { setWorkspaceError(errorMessage(reason)) }
    finally { setChoosing(false) }
  }
  async function submit(event: React.FormEvent) {
    event.preventDefault()
    if (!name.trim() || !workspace) { setWorkspaceError('请填写名称，并通过选择器授权工作区。'); return }
    setWorkspaceError(null)
    await onCreate({ name, provider, workspaceGrantId: workspace.id, securityMode, runtimeKind: runtime.kind })
  }
  return <div className="modal-backdrop" onMouseDown={onClose}><form className="modal-card form-modal" onSubmit={(event) => void submit(event)} onMouseDown={event => event.stopPropagation()}><div className="modal-header"><div><div className="section-kicker"><Sparkles size={13} /> New workspace</div><h2>Configure your agent</h2><p>Protected mode starts with no permissions.</p></div><button type="button" className="icon-button" onClick={onClose} aria-label="Close"><X size={18} /></button></div><label className="field-label">Agent name<input value={name} onChange={event => setName(event.target.value)} placeholder="e.g. Frontend Architect" maxLength={80} /></label><label className="field-label">Provider<select value={provider} onChange={event => setProvider(event.target.value as Provider)}>{providers.map(item => <option key={item.name}>{item.name}</option>)}</select></label><label className="field-label">Workspace<div className="workspace-field"><input value={workspace?.path ?? ''} readOnly placeholder={isDesktop ? 'Choose a local folder' : 'Choose a simulated workspace'} /><button type="button" className="quiet-button" disabled={choosing || isBusy} onClick={() => void chooseWorkspace()}>{choosing ? 'Choosing…' : 'Browse'}</button></div></label><label className="field-label">Security mode<select value={securityMode} onChange={event => setSecurityMode(event.target.value as 'Protected' | 'Developer')}><option value="Protected">Protected</option><option value="Developer">Developer</option></select></label><p>{runtime.message}</p>{workspaceError && <div className="field-error" role="alert">{workspaceError}</div>}<div className="modal-actions"><button type="button" className="quiet-button" onClick={onClose}>Cancel</button><button type="submit" className="primary-button" disabled={isBusy || choosing || !runtime.available}>{isBusy ? 'Creating…' : 'Create agent'}</button></div><div className="modal-note"><LockKeyhole size={14} /><span>{runtime.isolationEnforced ? 'Only the explicitly selected workspace is authorized.' : '模拟模式不会读取所选目录，也不会启动真实 Agent。'}</span></div></form></div>
}

function ApprovalModal({ approval, isBusy, onClose, onDecision }: { approval: Approval; isBusy: boolean; onClose: () => void; onDecision: (approval: Approval, allowed: boolean) => void }) { return <div className="modal-backdrop" onMouseDown={onClose}><div className="modal-card" onMouseDown={event => event.stopPropagation()}><div className="modal-header"><div><div className="section-kicker"><ShieldCheck size={13} /> Permission review</div><h2>{approval.title}</h2><p>Step-One will record your decision locally.</p></div><button className="icon-button" onClick={onClose} aria-label="Close"><X size={18} /></button></div><div className="approval-detail"><strong>{approval.detail.split('：')[0]}</strong><span>{approval.detail.split('：').slice(1).join('：')}</span></div><div className="modal-actions"><button className="quiet-button" disabled={isBusy} onClick={() => onDecision(approval, false)}>Deny</button><button className="primary-button" disabled={isBusy} onClick={() => onDecision(approval, true)}>Allow</button></div><div className="modal-note"><LockKeyhole size={14} /><span>Approval changes the policy for this Agent. It does not expose host credentials or grant arbitrary host access.</span></div></div></div> }
function LoadingState() { return <section className="panel loading-state"><div className="loading-bar" /><div className="loading-bar short" /><span>Loading security workspace…</span></section> }

export default App
