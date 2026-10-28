import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type { Backend, AppEvent, AppEventMap } from './index'
import type {
  ActivityRecord, BackupManifest, Capability, Diagnostics, RestorePlanSummary, SessionSnapshot,
  TrackingInfo,
} from '../types'

/**
 * Production backend: typed Tauri commands over the Rust core.
 * Every command name and payload matches src-tauri/src/commands.rs.
 */
export const tauriBackend: Backend = {
  kind: 'tauri',

  getState: () => invoke('get_state'),
  getCapabilities: () => invoke('get_capabilities'),
  getAppVersion: () => invoke('get_app_version'),
  getDiagnostics: () => invoke('get_diagnostics'),
  getCurrentActivity: () => invoke('get_current_activity'),

  getOverview: (date) => invoke('get_overview', { date }),
  getTimeline: (date) => invoke('get_timeline', { date }),
  getReport: (req) => invoke('get_report', { req }),
  getFocusHistory: (date) => invoke('get_focus_history', { date }),
  getProjectsWithTime: () => invoke('get_projects_with_time'),

  correctRecord: (id, cat, project, createRule) => invoke('correct_record', { id, cat, project, createRule }),
  splitRecord: (id, minutesInFirst) => invoke('split_record', { id, minutesInFirst }),
  undoLast: () => invoke('undo_last'),
  addRule: (app, cat, project) => invoke('add_rule', { app, cat, project }),
  applyRuleRetroactive: (index) => invoke('apply_rule_retroactive', { index }),
  addExclusion: (label) => invoke('add_exclusion', { label }),
  removeExclusion: (label) => invoke('remove_exclusion', { label }),
  deleteHistory: (start, end) => invoke('delete_history', { start, end }),

  startSession: (intention, project, focusMinutes, breakMinutes, breaksEnabled) => invoke('session_start', { intention, project, focusMinutes, breakMinutes, breaksEnabled }),
  resumeFocus: () => invoke('session_resume_focus'),
  extendBreak: () => invoke('session_extend_break'),
  toggleSession: () => invoke('session_toggle'),
  endSession: () => invoke('session_end'),

  setGoals: (goals) => invoke('set_goals', { goals }),
  createProject: (name, desc, client) => invoke('project_create', { name, desc, client }),
  renameProject: (id, name, desc, client) => invoke('project_rename', { id, name, desc, client }),
  setProjectArchived: (id, archived) => invoke('project_set_archived', { id, archived }),
  deleteProject: (id) => invoke('project_delete', { id }),
  bulkAssign: (ids, project) => invoke('bulk_assign', { ids, project }),

  manualEntryAdd: (startMs, endMs, label, cat, project) => invoke('manual_entry_add', { startMs, endMs, label, cat, project }),
  manualEntryUpdate: (id, startMs, endMs, label, cat, project) => invoke('manual_entry_update', { id, startMs, endMs, label, cat, project }),
  manualEntryDelete: (id) => invoke('manual_entry_delete', { id }),

  getWorkBlocks: (date) => invoke('get_work_blocks', { date }),
  groupBreakAdd: (atTs) => invoke('group_break_add', { atTs }),
  groupBreakRemove: (atTs) => invoke('group_break_remove', { atTs }),

  getIdleExceptions: () => invoke('get_idle_exceptions'),
  idleExceptionAdd: (label) => invoke('idle_exception_add', { label }),
  idleExceptionRemove: (id) => invoke('idle_exception_remove', { id }),

  getBlockList: () => invoke('get_block_list'),
  blockListAdd: (kind, target, label) => invoke('block_list_add', { kind, target, label }),
  blockListRemove: (id) => invoke('block_list_remove', { id }),
  blockListSetEnabled: (id, enabled) => invoke('block_list_set_enabled', { id, enabled }),

  retentionStatus: () => invoke('retention_status'),
  retentionSetActive: (active) => invoke('retention_set_active', { active }),
  retentionRunNow: () => invoke('retention_run_now'),

  pauseTracking: () => invoke('tracking_pause'),
  resumeTracking: () => invoke('tracking_resume'),
  enableTracking: () => invoke('tracking_enable'),
  disableTracking: () => invoke('tracking_disable'),
  simulateUnknownRecovery: () => invoke('tracking_simulate_unknown'),
  setPrivacy: (on) => invoke('set_privacy', { on }),

  setDnd: (on) => invoke('set_dnd', { on }),
  updateSettings: (patch) => invoke('update_settings', { patch }),
  completeOnboarding: (consent) => invoke('complete_onboarding', { consent }),

  createBackup: (dir) => invoke('backup_create', { dir }),
  inspectBackup: (path) => invoke<RestorePlanSummary>('backup_inspect', { path }),
  restoreBackup: (path, plan) => invoke('backup_restore', { path, plan }),
  exportData: (format, req, dir) => invoke('export_data', { format, req, dir }),
  quitApp: () => invoke('app_quit'),

  on<E extends AppEvent>(event: E, handler: (payload: AppEventMap[E]) => void): () => void {
    let unlisten: UnlistenFn | null = null
    let disposed = false
    listen<AppEventMap[E]>(`chronos://${event}`, (e) => handler(e.payload)).then((fn) => {
      if (disposed) fn()
      else unlisten = fn
    })
    return () => {
      disposed = true
      unlisten?.()
    }
  },
}

// Re-export types used by command wrappers
export type { ActivityRecord, BackupManifest, Capability, Diagnostics, SessionSnapshot, TrackingInfo }
