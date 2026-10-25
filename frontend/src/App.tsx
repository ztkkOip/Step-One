import { useEffect, useMemo, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import {
  Activity,
  ArrowUpRight,
  Bot,
  Check,
  ChevronRight,
  CircleHelp,
  Cloud,
  Command,
  Cpu,
  FileKey2,
  FolderLock,
  Gauge,
  KeyRound,
  LayoutDashboard,
  LockKeyhole,
  MessageSquareText,
  MoreHorizontal,
  Network,
  Plus,
  RotateCcw,
  Server,
  Settings2,
  ShieldCheck,
  Sparkles,
  TerminalSquare,
  Trash2,
  WifiOff,
  X,
  Zap,
} from 'lucide-react'

type AgentStatus = 'running' | 'ready' | 'stopped' | 'installing'
type ViewKey = 'overview' | 'agents' | 'connections' | 'audit'

type Agent = {
  id: string
  name: string
  provider: 'Claude Code' | 'Codex' | 'DeepSeek Harness'
  version: string
  status: AgentStatus
  workspace: string
  securityMode: 'Protected' | 'Developer'
  lastActivity: string
  accent: string
}

type AuditEvent = {
  time: string
  title: string
  detail: string
  tone: 'green' | 'blue' | 'amber'
}

const initialAgents: Agent[] = [
  {
    id: 'agent-frontend',
    name: 'Frontend Architect',
    provider: 'Claude Code',
    version: '1.0.84',
    status: 'running',
    workspace: 'D:/workspaces/step-one-ui',
    securityMode: 'Protected',
    lastActivity: '现在',
    accent: 'violet',
  },
  {
    id: 'agent-research',
    name: 'Research Runner',
    provider: 'Codex',
    version: '0.42.0',
    status: 'ready',
    workspace: 'D:/workspaces/research',
    securityMode: 'Protected',
    lastActivity: '12 分钟前',
    accent: 'blue',
  },
  {
    id: 'agent-local',
    name: 'Local Builder',
    provider: 'DeepSeek Harness',
    version: '0.8.2',
    status: 'stopped',
    workspace: 'D:/workspaces/local-builder',
    securityMode: 'Developer',
    lastActivity: '昨天',
    accent: 'orange',
  },
]

const initialAudit: AuditEvent[] = [
  { time: '10:42:18', title: 'Agent session started', detail: 'Frontend Architect · Protected mode', tone: 'green' },
  { time: '10:41:52', title: 'Workspace mounted', detail: 'step-one-ui · read/write', tone: 'blue' },
  { time: '10:39:04', title: 'Network request blocked', detail: 'connectivity disabled by policy', tone: 'amber' },
  { time: '10:36:47', title: 'Credential broker checked', detail: 'no secret exposed to sandbox', tone: 'green' },
]

const navItems: Array<{ key: ViewKey; label: string; icon: typeof LayoutDashboard }> = [
  { key: 'overview', label: 'Overview', icon: LayoutDashboard },
  { key: 'agents', label: 'Agents', icon: Bot },
  { key: 'connections', label: 'MCP & Connections', icon: Network },
  { key: 'audit', label: 'Audit log', icon: Activity },
]

function statusLabel(status: AgentStatus) {
  return {
    running: 'Running',
    ready: 'Ready',
    stopped: 'Stopped',
    installing: 'Installing',
  }[status]
}

function App() {
  const [activeView, setActiveView] = useState<ViewKey>('overview')
  const [agents, setAgents] = useState(initialAgents)
  const [activeAgentId, setActiveAgentId] = useState(initialAgents[0].id)
  const [auditEvents, setAuditEvents] = useState(initialAudit)
  const [isCreateOpen, setIsCreateOpen] = useState(false)
  const [isCreating, setIsCreating] = useState(false)
  const [toast, setToast] = useState<string | null>(null)

  const activeAgent = agents.find((agent) => agent.id === activeAgentId) ?? agents[0]
  const runningCount = agents.filter((agent) => agent.status === 'running').length
  const protectedCount = agents.filter((agent) => agent.securityMode === 'Protected').length

  useEffect(() => {
    void invoke<Agent[]>('list_agents')
      .then((remoteAgents) => {
        if (remoteAgents.length > 0) setAgents(remoteAgents)
      })
      .catch(() => undefined)
  }, [])

  useEffect(() => {
    if (!toast) return
    const timer = window.setTimeout(() => setToast(null), 3200)
    return () => window.clearTimeout(timer)
  }, [toast])

  const pageTitle = useMemo(() => {
    const item = navItems.find((navItem) => navItem.key === activeView)
    return item?.label ?? 'Overview'
  }, [activeView])

  function recordAudit(title: string, detail: string, tone: AuditEvent['tone'] = 'green') {
    setAuditEvents((current) => [
      { time: new Date().toLocaleTimeString('zh-CN', { hour12: false }), title, detail, tone },
      ...current,
    ])
  }

  async function handleAgentAction(action: 'start' | 'stop' | 'reset') {
    if (!activeAgent) return
    const nextStatus: AgentStatus = action === 'start' ? 'running' : action === 'stop' ? 'stopped' : 'ready'
    setAgents((current) => current.map((agent) => agent.id === activeAgent.id ? { ...agent, status: nextStatus, lastActivity: '刚刚' } : agent))
    const actionLabels = { start: 'started', stop: 'stopped', reset: 'reset' }
    recordAudit(`Agent ${actionLabels[action]}`, `${activeAgent.name} · simulator runtime`, action === 'reset' ? 'blue' : 'green')
    setToast(`${activeAgent.name} 已${action === 'start' ? '启动' : action === 'stop' ? '停止' : '重置'}`)

    try {
      await invoke('agent_action', { agentId: activeAgent.id, action })
    } catch {
      // Browser preview uses the local simulator state until Tauri is running.
    }
  }

  async function handleCreateAgent(provider: Agent['provider']) {
    setIsCreating(true)
    const id = `agent-${Date.now()}`
    const newAgent: Agent = {
      id,
      name: provider === 'Claude Code' ? 'New Claude Workspace' : provider === 'Codex' ? 'New Codex Workspace' : 'New Harness Workspace',
      provider,
      version: provider === 'Claude Code' ? '1.0.84' : provider === 'Codex' ? '0.42.0' : '0.8.2',
      status: 'installing',
      workspace: '选择工作区后显示',
      securityMode: 'Protected',
      lastActivity: '刚刚',
      accent: provider === 'Claude Code' ? 'violet' : provider === 'Codex' ? 'blue' : 'orange',
    }

    setAgents((current) => [newAgent, ...current])
    setActiveAgentId(id)
    setIsCreateOpen(false)
    recordAudit('Agent provisioning started', `${provider} · Protected mode`, 'blue')
    setToast(`${provider} 正在准备安全环境`)

    try {
      await invoke('create_agent', { provider })
    } catch {
      // The simulator completes provisioning in the browser preview.
    }

    window.setTimeout(() => {
      setAgents((current) => current.map((agent) => agent.id === id ? { ...agent, status: 'ready' } : agent))
      recordAudit('Agent ready', `${newAgent.name} · no credentials in sandbox`, 'green')
      setToast(`${newAgent.name} 已准备完成`)
      setIsCreating(false)
    }, 850)
  }

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand-lockup">
          <div className="brand-mark"><ShieldCheck size={19} strokeWidth={2.5} /></div>
          <div>
            <div className="brand-name">step-one</div>
            <div className="brand-caption">secure agent workspace</div>
          </div>
        </div>

        <div className="workspace-switcher">
          <span className="workspace-avatar">Z</span>
          <div className="workspace-copy">
            <span className="workspace-label">Workspace</span>
            <strong>Personal lab</strong>
          </div>
          <ChevronRight size={15} className="muted-icon" />
        </div>

        <div className="nav-section-label">Workspace</div>
        <nav className="main-nav" aria-label="Main navigation">
          {navItems.map(({ key, label, icon: Icon }) => (
            <button key={key} className={`nav-item ${activeView === key ? 'active' : ''}`} onClick={() => setActiveView(key)}>
              <Icon size={17} />
              <span>{label}</span>
              {key === 'agents' && <span className="nav-count">{agents.length}</span>}
            </button>
          ))}
        </nav>

        <div className="nav-section-label">Manage</div>
        <nav className="main-nav">
          <button className="nav-item" onClick={() => setToast('设置中心将在凭据代理 PR 中启用')}><Settings2 size={17} /><span>Settings</span></button>
          <button className="nav-item" onClick={() => setToast('帮助中心正在准备中')}><CircleHelp size={17} /><span>Help center</span></button>
        </nav>

        <div className="sidebar-bottom">
          <div className="host-status">
            <span className="status-dot green" />
            <div><strong>Host protected</strong><span>Windows 11 · simulator</span></div>
            <MoreHorizontal size={16} className="muted-icon" />
          </div>
          <div className="user-row">
            <div className="user-avatar">ZT</div>
            <div className="workspace-copy"><strong>ztkkOip</strong><span>Local account</span></div>
            <ChevronRight size={15} className="muted-icon" />
          </div>
        </div>
      </aside>

      <main className="main-content">
        <header className="topbar">
          <div className="breadcrumb"><span>Personal lab</span><ChevronRight size={13} /><strong>{pageTitle}</strong></div>
          <div className="topbar-actions">
            <div className="runtime-chip"><span className="pulse-dot" />Simulator runtime</div>
            <button className="icon-button" onClick={() => setToast('当前已是最新安全状态')} aria-label="Notifications"><Activity size={17} /></button>
            <button className="avatar-button">ZT</button>
          </div>
        </header>

        <div className="content-wrap">
          <section className="welcome-row">
            <div>
              <div className="section-kicker"><Sparkles size={13} /> Security workspace</div>
              <h1>{activeView === 'overview' ? 'Good morning, ZT.' : pageTitle}</h1>
              <p>{activeView === 'overview' ? 'Your agents are ready. Everything outside their permissions stays outside.' : 'Manage your local agent environment with clear, human-readable controls.'}</p>
            </div>
            <button className="primary-button" onClick={() => setIsCreateOpen(true)}><Plus size={17} /> New agent</button>
          </section>

          {activeView === 'overview' && (
            <>
              <section className="hero-card">
                <div className="hero-copy">
                  <div className="hero-badge"><LockKeyhole size={14} /> Protected by default</div>
                  <h2>Build with confidence.<br /><em>Stay in control.</em></h2>
                  <p>Every agent runs in its own workspace. Your credentials stay on the host, and permissions are visible before anything starts.</p>
                  <button className="secondary-button" onClick={() => setActiveView('connections')}>Review security model <ArrowUpRight size={15} /></button>
                </div>
                <div className="hero-visual" aria-hidden="true">
                  <div className="orbit orbit-one" /><div className="orbit orbit-two" />
                  <div className="core-shield"><ShieldCheck size={42} strokeWidth={1.45} /></div>
                  <div className="orbit-node node-one"><KeyRound size={15} /></div>
                  <div className="orbit-node node-two"><FolderLock size={15} /></div>
                  <div className="orbit-node node-three"><WifiOff size={15} /></div>
                </div>
              </section>

              <section className="stats-grid">
                <StatCard label="Active agents" value={String(runningCount)} detail={`of ${agents.length} workspaces`} icon={<Bot size={18} />} tone="violet" />
                <StatCard label="Protected workspaces" value={String(protectedCount)} detail="credentials isolated" icon={<LockKeyhole size={18} />} tone="green" />
                <StatCard label="Blocked today" value="12" detail="network requests" icon={<WifiOff size={18} />} tone="orange" />
                <StatCard label="Host health" value="Good" detail="all systems normal" icon={<Gauge size={18} />} tone="blue" />
              </section>
            </>
          )}

          {activeView === 'connections' ? <ConnectionsView onToast={setToast} /> : activeView === 'audit' ? <AuditView events={auditEvents} /> : <>
            <section className="section-header"><div><h2>{activeView === 'agents' ? 'All agents' : 'Your agents'}</h2><p>{activeView === 'agents' ? 'Each workspace has its own security boundary.' : 'Pick up where you left off.'}</p></div><button className="quiet-button" onClick={() => setIsCreateOpen(true)}><Plus size={15} /> Add agent</button></section>
            <section className="agents-grid">
              {agents.map((agent) => <AgentCard key={agent.id} agent={agent} selected={agent.id === activeAgentId} onSelect={() => setActiveAgentId(agent.id)} />)}
              <button className="add-agent-card" onClick={() => setIsCreateOpen(true)}><span><Plus size={20} /></span><strong>Create an agent</strong><small>Start from a protected template</small></button>
            </section>

            <section className="lower-grid">
              <div className="panel activity-panel"><div className="panel-heading"><div><h2>Recent activity</h2><p>Last actions across your workspace.</p></div><button className="text-button" onClick={() => setActiveView('audit')}>View all <ArrowUpRight size={14} /></button></div><div className="activity-list">{auditEvents.slice(0, 4).map((event, index) => <ActivityRow key={`${event.time}-${index}`} event={event} />)}</div></div>
              <div className="panel selected-panel"><div className="panel-heading"><div><h2>Selected agent</h2><p>Quick controls for this workspace.</p></div><span className={`status-pill ${activeAgent?.status}`}>{activeAgent ? statusLabel(activeAgent.status) : '—'}</span></div>{activeAgent ? <><div className="selected-agent"><div className={`agent-logo ${activeAgent.accent}`}><Bot size={21} /></div><div><strong>{activeAgent.name}</strong><span>{activeAgent.provider} · v{activeAgent.version}</span></div></div><div className="permission-summary"><PermissionItem icon={<FolderLock size={15} />} label="Workspace" value={activeAgent.workspace === '选择工作区后显示' ? 'Not mounted' : 'Read / write'} /><PermissionItem icon={<WifiOff size={15} />} label="Network" value="Blocked by default" /><PermissionItem icon={<FileKey2 size={15} />} label="Credentials" value="Host only" /></div><div className="control-row"><button className="control-button primary" disabled={isCreating || activeAgent.status === 'running'} onClick={() => void handleAgentAction('start')}><Zap size={15} /> Start</button><button className="control-button" disabled={activeAgent.status !== 'running'} onClick={() => void handleAgentAction('stop')}><TerminalSquare size={15} /> Stop</button><button className="control-button icon-only" onClick={() => void handleAgentAction('reset')} aria-label="Reset agent"><RotateCcw size={15} /></button></div></> : <div className="empty-selection">Select an agent to view controls.</div>}</div>
            </section>
          </>}
        </div>
      </main>

      {isCreateOpen && <CreateAgentModal onClose={() => setIsCreateOpen(false)} onCreate={(provider) => void handleCreateAgent(provider)} />}
      {toast && <div className="toast"><span className="toast-check"><Check size={14} /></span>{toast}<button onClick={() => setToast(null)} aria-label="Dismiss"><X size={14} /></button></div>}
    </div>
  )
}

function StatCard({ label, value, detail, icon, tone }: { label: string; value: string; detail: string; icon: React.ReactNode; tone: string }) {
  return <div className="stat-card"><div className={`stat-icon ${tone}`}>{icon}</div><div className="stat-copy"><span>{label}</span><strong>{value}</strong><small>{detail}</small></div><ArrowUpRight size={15} className="stat-arrow" /></div>
}

function AgentCard({ agent, selected, onSelect }: { agent: Agent; selected: boolean; onSelect: () => void }) {
  return <button className={`agent-card ${selected ? 'selected' : ''}`} onClick={onSelect}><div className="agent-card-top"><div className={`agent-logo ${agent.accent}`}><Bot size={22} /></div><span className={`status-pill ${agent.status}`}><span className="status-dot" />{statusLabel(agent.status)}</span></div><div className="agent-card-copy"><strong>{agent.name}</strong><span>{agent.provider} · v{agent.version}</span></div><div className="agent-card-footer"><span><FolderLock size={13} /> {agent.securityMode} mode</span><ChevronRight size={15} /></div></button>
}

function PermissionItem({ icon, label, value }: { icon: React.ReactNode; label: string; value: string }) {
  return <div className="permission-item"><span className="permission-icon">{icon}</span><div><span>{label}</span><strong>{value}</strong></div><span className="permission-check"><Check size={12} /></span></div>
}

function ActivityRow({ event }: { event: AuditEvent }) {
  return <div className="activity-row"><span className={`activity-dot ${event.tone}`} /><div className="activity-copy"><strong>{event.title}</strong><span>{event.detail}</span></div><time>{event.time}</time></div>
}

function AuditView({ events }: { events: AuditEvent[] }) {
  return <section className="panel full-panel"><div className="panel-heading"><div><h2>Audit log</h2><p>Every sandbox decision is recorded on the host.</p></div><div className="secure-label"><ShieldCheck size={14} /> Local only</div></div><div className="activity-list audit-list">{events.map((event, index) => <ActivityRow key={`${event.time}-${index}`} event={event} />)}</div></section>
}

function ConnectionsView({ onToast }: { onToast: (message: string) => void }) {
  const connections = [{ name: 'Model proxy', description: 'Claude, Codex and DeepSeek access', icon: Cloud, state: 'Ready', tone: 'green' }, { name: 'GitHub', description: 'OAuth connection for repository tools', icon: Command, state: 'Not connected', tone: 'muted' }, { name: 'MCP Gateway', description: 'Tool permissions stay inside policy', icon: Server, state: 'Simulator', tone: 'blue' }]
  return <section className="panel full-panel connections-panel"><div className="panel-heading"><div><h2>MCP & Connections</h2><p>Connect capabilities without placing secrets inside an agent.</p></div><div className="secure-label"><LockKeyhole size={14} /> Secrets stay on host</div></div><div className="connection-list">{connections.map(({ name, description, icon: Icon, state, tone }) => <div className="connection-row" key={name}><div className="connection-icon"><Icon size={19} /></div><div className="connection-copy"><strong>{name}</strong><span>{description}</span></div><span className={`connection-state ${tone}`}>{state}</span><button className="quiet-button" onClick={() => onToast(`${name} 的配置将在后续安全连接 PR 中启用`)}>{state === 'Not connected' ? 'Connect' : 'Manage'} <ChevronRight size={14} /></button></div>)}</div></section>
}

function CreateAgentModal({ onClose, onCreate }: { onClose: () => void; onCreate: (provider: Agent['provider']) => void }) {
  const providers: Array<{ name: Agent['provider']; description: string; version: string; icon: string }> = [{ name: 'Claude Code', description: 'Anthropic coding agent', version: '1.0.84', icon: 'CC' }, { name: 'Codex', description: 'OpenAI coding agent', version: '0.42.0', icon: 'CX' }, { name: 'DeepSeek Harness', description: 'DeepSeek agent runner', version: '0.8.2', icon: 'DS' }]
  return <div className="modal-backdrop" onMouseDown={onClose}><div className="modal-card" onMouseDown={(event) => event.stopPropagation()}><div className="modal-header"><div><div className="section-kicker"><Sparkles size={13} /> New workspace</div><h2>Choose your agent</h2><p>It will start in Protected mode with no network access.</p></div><button className="icon-button" onClick={onClose} aria-label="Close"><X size={18} /></button></div><div className="provider-list">{providers.map((provider) => <button className="provider-option" key={provider.name} onClick={() => onCreate(provider.name)}><span className="provider-mark">{provider.icon}</span><span className="provider-copy"><strong>{provider.name}</strong><small>{provider.description} · v{provider.version}</small></span><ChevronRight size={16} /></button>)}</div><div className="modal-note"><LockKeyhole size={14} /><span>API keys and credentials remain on your host. This preview uses a simulator runtime.</span></div></div></div>
}

export default App
