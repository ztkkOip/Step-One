import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { Agent, Snapshot } from './types'
import { deniedPermissions } from './types'
const api = vi.hoisted(() => ({ load: vi.fn(), chooseWorkspace: vi.fn(), create: vi.fn(), action: vi.fn(), request: vi.fn(), decide: vi.fn(), revoke: vi.fn() }))
vi.mock('./consoleApi', () => ({ consoleApi: api, isDesktop: true, errorMessage: (error: unknown) => error instanceof Error ? error.message : (error as { message: string }).message }))
import App from './App'

function agent(overrides: Partial<Agent> = {}): Agent {
  return { id: 'server-id', name: 'Backend Agent', provider: 'Codex', version: 'not-installed', status: 'ready', workspace: '/workspace', securityMode: 'Protected', permissions: deniedPermissions(), lastActivity: 'now', accent: 'blue', runtimeKind: 'simulator', runtimeState: 'ready', operationId: null, lastError: null, resourceId: 'resource', ...overrides }
}
function state(agents: Agent[] = []): Snapshot {
  return { agents, events: [], approvals: [], operations: [], runtime: { kind: 'simulator', available: true, isolationEnforced: false, capabilities: { workspaceWrite: true, network: true, mcpTools: true, clipboard: true, exportFiles: true }, message: 'SIMULATOR: no real isolation' } }
}
beforeEach(() => { vi.clearAllMocks(); api.load.mockResolvedValue(state()) })
async function openCreate() {
  const user = userEvent.setup()
  render(<App />)
  await screen.findByText('Create an agent to view controls.')
  await user.click(screen.getByRole('button', { name: 'New agent' }))
  return user
}

describe('console runtime UI', () => {
  it('shows the empty state and explicit simulator boundary without fictitious agents', async () => {
    render(<App />)
    await screen.findByText('Create an agent to view controls.')
    expect(screen.queryByText('Frontend Architect')).not.toBeInTheDocument()
    expect(screen.getByText('当前没有真实隔离')).toBeInTheDocument()
    expect(screen.getByText('SIMULATOR: no real isolation')).toBeInTheDocument()
    expect(screen.queryByText('Host protected')).not.toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: 'Audit log' }))
    expect(screen.getByText('No audit events yet.')).toBeInTheDocument()
  })
  it('adopts backend IDs and workspace grants for subsequent operations', async () => {
    const user = await openCreate()
    api.chooseWorkspace.mockResolvedValue({ id: 'host-grant', path: 'C:/Chosen' })
    api.create.mockResolvedValue(state([agent()]))
    api.action.mockResolvedValue(state([agent({ status: 'running', runtimeState: 'running' })]))
    await user.type(screen.getByLabelText('Agent name'), 'Typed name')
    await user.selectOptions(screen.getByLabelText('Provider'), 'Codex')
    await user.selectOptions(screen.getByLabelText('Security mode'), 'Developer')
    await user.click(screen.getByRole('button', { name: 'Browse' }))
    expect(screen.getByDisplayValue('C:/Chosen')).toHaveAttribute('readonly')
    await user.click(screen.getByRole('button', { name: 'Create agent' }))
    expect(api.create).toHaveBeenCalledWith({ name: 'Typed name', provider: 'Codex', workspaceGrantId: 'host-grant', securityMode: 'Developer', runtimeKind: 'simulator' })
    await user.click(await screen.findByRole('button', { name: 'Start' }))
    expect(api.action).toHaveBeenCalledWith('server-id', 'start')
    expect(screen.getByRole('button', { name: 'Start' })).toBeDisabled()
  })
  it('keeps state on failure and provides refresh without reporting success', async () => {
    api.load.mockResolvedValue(state([agent()]))
    api.action.mockRejectedValue({ code: 'DENIED', message: 'Execution denied' })
    render(<App />)
    await userEvent.click(await screen.findByRole('button', { name: 'Start' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('Execution denied')
    expect(screen.queryByText('Backend Agent 已启动')).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Stop' })).toBeDisabled()
    api.load.mockResolvedValue(state())
    await userEvent.click(screen.getByRole('button', { name: 'Refresh' }))
    expect(await screen.findByText('Create an agent to view controls.')).toBeInTheDocument()
    expect(screen.queryByText('Backend Agent')).not.toBeInTheDocument()
  })
  it('reports an initial IPC error, never preview agents', async () => {
    api.load.mockRejectedValue(new Error('Database unavailable'))
    render(<App />)
    expect(await screen.findByRole('alert')).toHaveTextContent('Database unavailable')
    expect(screen.queryByText('Frontend Architect')).not.toBeInTheDocument()
    await userEvent.click(screen.getByLabelText('Dismiss error'))
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
  })
  it('disables pending actions and polls authoritative snapshots until completion', async () => {
    const pending = state([agent({ runtimeState: 'starting', operationId: 'op' })])
    pending.operations = [{ id: 'op', agentId: 'server-id', action: 'start', state: 'pending', error: null }]
    api.load.mockResolvedValueOnce(pending).mockResolvedValue(state([agent({ status: 'running', runtimeState: 'running' })]))
    render(<App />)
    expect(await screen.findByText('运行环境：starting · 操作处理中')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Start' })).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Reset' })).toBeDisabled()
    await waitFor(() => expect(screen.getByRole('button', { name: 'Stop' })).toBeEnabled(), { timeout: 3000 })
  })
  it('never claims completion when an operation is only accepted', async () => {
    api.load.mockResolvedValue(state([agent()]))
    const pending = state([agent({ runtimeState: 'starting', operationId: 'op' })])
    pending.operations = [{ id: 'op', agentId: 'server-id', action: 'start', state: 'pending', error: null }]
    api.action.mockResolvedValue(pending)
    render(<App />)
    await userEvent.click(await screen.findByRole('button', { name: 'Start' }))
    expect(await screen.findByText('操作已提交，正在等待运行环境确认。')).toBeInTheDocument()
    expect(screen.queryByText('Backend Agent 已启动')).not.toBeInTheDocument()
  })
  it('disables unsupported capabilities and shows runtime errors', async () => {
    const snapshot = state([agent({ runtimeState: 'failed', lastError: { code: 'TIMEOUT', message: 'Start timed out', retryable: true, operationId: 'op' } })])
    snapshot.runtime = { ...snapshot.runtime, kind: 'hyperv', available: false, capabilities: deniedPermissions(), message: 'Hyper-V unavailable' }
    api.load.mockResolvedValue(snapshot)
    render(<App />)
    expect(await screen.findByRole('alert')).toHaveTextContent('Start timed out')
    expect(screen.getByRole('button', { name: 'Start' })).toBeDisabled()
    screen.getAllByRole('button', { name: 'Request' }).forEach(button => expect(button).toBeDisabled())
    expect(screen.getByText('Hyper-V unavailable')).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: 'New agent' }))
    expect(screen.getByRole('button', { name: 'Create agent' })).toBeDisabled()
  })
  it('allows only stopping an unknown recovered process before editing its policy', async () => {
    api.load.mockResolvedValue(state([agent({ status: 'unknown', runtimeState: 'failed' })]))
    api.action.mockResolvedValue(state([agent({ status: 'stopped', runtimeState: 'stopped' })]))
    render(<App />)
    expect(await screen.findByRole('button', { name: 'Stop' })).toBeEnabled()
    expect(screen.getByRole('button', { name: 'Start' })).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Reset' })).toBeDisabled()
    expect(screen.getByLabelText('Delete agent')).toBeDisabled()
    screen.getAllByRole('button', { name: 'Request' }).forEach(button => expect(button).toBeDisabled())
    await userEvent.click(screen.getByRole('button', { name: 'Stop' }))
    expect(api.action).toHaveBeenCalledWith('server-id', 'stop')
    expect(screen.getByRole('button', { name: 'Start' })).toBeEnabled()
  })
  it('handles workspace picker cancellation and errors without an arbitrary path fallback', async () => {
    const user = await openCreate()
    api.chooseWorkspace.mockResolvedValueOnce(null).mockRejectedValueOnce({ message: 'Folder rejected' })
    await user.click(screen.getByRole('button', { name: 'Browse' }))
    await user.click(screen.getByRole('button', { name: 'Create agent' }))
    expect(screen.getByRole('alert')).toHaveTextContent('请填写名称')
    await user.click(screen.getByRole('button', { name: 'Browse' }))
    expect(screen.getByRole('alert')).toHaveTextContent('Folder rejected')
    expect(api.create).not.toHaveBeenCalled()
    await user.click(screen.getByRole('button', { name: 'Cancel' }))
    expect(screen.queryByText('Configure your agent')).not.toBeInTheDocument()
  })
  it('keeps the create dialog and shows errors when creation fails', async () => {
    const user = await openCreate()
    api.chooseWorkspace.mockResolvedValue({ id: 'host-grant', path: 'C:/Chosen' })
    api.create.mockRejectedValue({ message: 'Creation failed' })
    await user.type(screen.getByLabelText('Agent name'), 'X')
    await user.click(screen.getByRole('button', { name: 'Browse' }))
    await user.click(screen.getByRole('button', { name: 'Create agent' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('Creation failed')
    expect(screen.getByText('Configure your agent')).toBeInTheDocument()
    await user.click(screen.getByLabelText('Close'))
  })
  it('uses approval responses for permission changes, then revocation and deletion', async () => {
    api.load.mockResolvedValue(state([agent()]))
    render(<App />)
    await screen.findByRole('button', { name: 'Start' })
    const requested = state([agent()])
    requested.approvals = [{ id: 'approval', agentId: 'server-id', action: 'network', title: 'Network review', detail: 'Backend Agent：Allow network?' }]
    api.request.mockResolvedValue(requested)
    const granted = state([agent({ permissions: { ...deniedPermissions(), network: true } })])
    api.decide.mockResolvedValue(granted)
    await userEvent.click(screen.getAllByRole('button', { name: 'Request' })[1])
    await userEvent.click(await screen.findByRole('button', { name: 'Allow' }))
    expect(api.decide).toHaveBeenCalledWith('approval', true)
    expect(screen.getByText('Simulated allowance')).toBeInTheDocument()
    api.revoke.mockResolvedValue(state([agent()]))
    await userEvent.click(screen.getByRole('button', { name: 'Revoke' }))
    expect(api.revoke).toHaveBeenCalledWith('server-id', 'network')
    requested.approvals[0].action = 'delete'
    requested.approvals[0].title = 'Delete review'
    api.request.mockResolvedValue(requested)
    api.decide.mockResolvedValue(state())
    await userEvent.click(screen.getByLabelText('Delete agent'))
    await userEvent.click(await screen.findByRole('button', { name: 'Allow' }))
    expect(await screen.findByText('Create an agent to view controls.')).toBeInTheDocument()
  })
  it('allows denying and reopening review, selecting agents, stopping and requesting reset', async () => {
    const initial = state([agent({ status: 'running', runtimeState: 'running' }), agent({ id: 'second', name: 'Second' })])
    initial.events = [{ id: 'event', title: 'Existing event', time: 'now', detail: 'Audit data' }]
    api.load.mockResolvedValue(initial)
    api.action.mockResolvedValue(state([agent(), agent({ id: 'second', name: 'Second' })]))
    render(<App />)
    await userEvent.click(await screen.findByRole('button', { name: 'Stop' }))
    expect(api.action).toHaveBeenCalledWith('server-id', 'stop')
    api.load.mockResolvedValue(state([agent(), agent({ id: 'second', name: 'Second' })]))
    await userEvent.click(screen.getByRole('button', { name: 'Refresh' }))
    await userEvent.click(screen.getByRole('button', { name: /Second/ }))
    const requested = state([agent(), agent({ id: 'second', name: 'Second' })])
    requested.approvals = [{ id: 'reset-review', agentId: 'second', action: 'reset', title: 'Reset review', detail: 'Second：Reset?' }]
    api.request.mockResolvedValue(requested)
    api.decide.mockResolvedValue(state([agent()]))
    await userEvent.click(screen.getByRole('button', { name: 'Reset' }))
    await userEvent.click(await screen.findByLabelText('Close'))
    await userEvent.click(screen.getByRole('button', { name: 'Review' }))
    await userEvent.click(screen.getByRole('button', { name: 'Deny' }))
    expect(api.decide).toHaveBeenCalledWith('reset-review', false)
  })
})
