import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { CreateAgentInput } from './types'
const tauri = vi.hoisted(() => ({ desktop: false, invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ isTauri: () => tauri.desktop, invoke: tauri.invoke }))

beforeEach(() => { tauri.desktop = false; tauri.invoke.mockReset(); vi.resetModules() })

async function setup() {
  const { createPreviewConsole } = await import('./consoleApi')
  const api = createPreviewConsole()
  const grant = await api.chooseWorkspace()
  const input: CreateAgentInput = { name: 'My agent', provider: 'Codex', workspaceGrantId: grant.id, securityMode: 'Protected' }
  const result = await api.create(input)
  return { api, input, id: result.agents[0].id }
}

describe('preview workflow', () => {
  it('is empty and explicitly unenforced, with isolated snapshots', async () => {
    const { createPreviewConsole } = await import('./consoleApi')
    const api = createPreviewConsole()
    const state = await api.load()
    expect(state.agents).toEqual([])
    expect(state.runtime.isolationEnforced).toBe(false)
    state.runtime.available = false
    expect((await api.load()).runtime.available).toBe(true)
  })
  it('runs the approval, start, stop, revoke, reset and deletion golden path', async () => {
    const { api, id } = await setup()
    expect((await api.load()).agents[0].permissions.network).toBe(false)
    let state = await api.request(id, 'network')
    const approval = state.approvals[0].id
    await expect(api.action(id, 'start')).rejects.toThrow('待确认')
    await expect(api.request(id, 'reset')).rejects.toThrow('待确认')
    await expect(api.revoke(id, 'network')).rejects.toThrow('待确认')
    state = await api.decide(approval, true)
    expect(state.agents[0].permissions.network).toBe(true)
    await expect(api.decide(approval, true)).rejects.toThrow('不存在')
    await expect(api.request(id, 'network')).rejects.toThrow('已经允许')
    state = await api.action(id, 'start')
    expect(state.agents[0].runtimeState).toBe('running')
    await expect(api.action(id, 'start')).rejects.toThrow('当前状态')
    await expect(api.request(id, 'reset')).rejects.toThrow('停止')
    await api.action(id, 'stop')
    await api.revoke(id, 'network')
    state = await api.request(id, 'workspaceWrite')
    await api.decide(state.approvals[0].id, true)
    state = await api.request(id, 'reset')
    state = await api.decide(state.approvals[0].id, true)
    expect(Object.values(state.agents[0].permissions).every(value => !value)).toBe(true)
    expect(state.agents[0].runtimeState).toBe('ready')
    state = await api.request(id, 'delete')
    state = await api.decide(state.approvals[0].id, false)
    expect(state.agents).toHaveLength(1)
    state = await api.request(id, 'delete')
    state = await api.decide(state.approvals[0].id, true)
    expect(state.agents).toEqual([])
    expect(state.events.length).toBeGreaterThan(0)
    await expect(api.action(id, 'start')).rejects.toThrow('不存在')
  })
  it.each(['Claude Code', 'Codex', 'DeepSeek Harness'] as const)('creates %s without claiming installation', async provider => {
    const { api, input } = await setup()
    const state = await api.create({ ...input, provider })
    expect(state.agents[0].version).toBe('not-installed')
    expect(state.agents[0].provider).toBe(provider)
  })
  it('rejects unknown grants, runtime and invalid input', async () => {
    const { api, input, id } = await setup()
    await expect(api.create({ ...input, runtimeKind: 'hyperv' })).rejects.toThrow('不可用')
    await expect(api.create({ ...input, name: ' ' })).rejects.toThrow('1–80')
    await expect(api.create({ ...input, name: 'x'.repeat(81) })).rejects.toThrow('1–80')
    await expect(api.create({ ...input, workspaceGrantId: 'forged' })).rejects.toThrow('重新选择')
    await expect(api.create({ ...input, provider: 'unknown' as never })).rejects.toThrow('类型')
    await expect(api.create({ ...input, securityMode: 'unknown' as never })).rejects.toThrow('模式')
    await expect(api.request(id, 'shell' as never)).rejects.toThrow('不支持')
    await expect(api.revoke(id, 'shell' as never)).rejects.toThrow('未知')
    await expect(api.action(id, 'stop')).rejects.toThrow('当前状态')
  })
})

describe('desktop IPC boundary', () => {
  it('forwards only grant based creation and exact commands; returns backend snapshots', async () => {
    tauri.desktop = true
    const { consoleApi } = await import('./consoleApi')
    const expected = { agents: [{ id: 'backend-id' }] }
    tauri.invoke.mockResolvedValue(expected)
    expect(await consoleApi.load()).toBe(expected)
    expect(await consoleApi.runtimeCapabilities()).toBe(expected)
    await consoleApi.chooseWorkspace()
    const input: CreateAgentInput = { name: 'A', provider: 'Codex', securityMode: 'Protected', workspaceGrantId: 'grant' }
    expect(await consoleApi.create(input)).toBe(expected)
    await consoleApi.action('backend-id', 'start')
    await consoleApi.request('backend-id', 'network')
    await consoleApi.decide('approval', false)
    await consoleApi.revoke('backend-id', 'network')
    expect(tauri.invoke.mock.calls).toEqual([
      ['console_snapshot'], ['runtime_capabilities'], ['choose_workspace'], ['create_agent', { input }],
      ['agent_action', { agentId: 'backend-id', action: 'start' }],
      ['request_approval', { agentId: 'backend-id', action: 'network' }],
      ['decide_approval', { approvalId: 'approval', allowed: false }],
      ['revoke_permission', { agentId: 'backend-id', key: 'network' }],
    ])
  })
  it('propagates errors and never falls back to the preview', async () => {
    tauri.desktop = true
    const { consoleApi } = await import('./consoleApi')
    const failure = { code: 'RUNTIME_UNAVAILABLE', message: 'Hyper-V unavailable', retryable: false }
    tauri.invoke.mockRejectedValue(failure)
    await expect(consoleApi.load()).rejects.toEqual(failure)
    await expect(consoleApi.action('anything', 'start')).rejects.toEqual(failure)
  })
  it('routes browser API through explicit preview', async () => {
    const { consoleApi } = await import('./consoleApi')
    expect((await consoleApi.runtimeCapabilities()).available).toBe(false)
    const grant = await consoleApi.chooseWorkspace()
    let state = await consoleApi.create({ name: 'UI', provider: 'Codex', securityMode: 'Protected', workspaceGrantId: grant!.id })
    const id = state.agents[0].id
    await consoleApi.action(id, 'start')
    await consoleApi.action(id, 'stop')
    state = await consoleApi.request(id, 'network')
    await consoleApi.decide(state.approvals[0].id, true)
    await consoleApi.revoke(id, 'network')
    expect((await consoleApi.load()).agents[0].permissions.network).toBe(false)
    expect(tauri.invoke).not.toHaveBeenCalled()
  })
  it('formats only useful error messages', async () => {
    const { errorMessage } = await import('./consoleApi')
    expect(errorMessage(new Error('error'))).toBe('error')
    expect(errorMessage({ message: 'structured' })).toBe('structured')
    expect(errorMessage('plain')).toBe('plain')
    expect(errorMessage(null)).toBe('操作失败，请重试。')
    expect(errorMessage({ message: 3 })).toBe('操作失败，请重试。')
  })
})
