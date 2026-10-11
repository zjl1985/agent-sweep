export type Locale = "en" | "zh";

export interface Strings {
  reclaimStorage: string;
  reclaimDescription: string;
  closeOpenCode: string;
  openCodeDeleteNote: string;
  sessionDataNote: string;
  reclaimedNotice: (count: number, size: string) => string;
  compareUsage: string;
  agentsCombined: (count: number) => string;
  brandSubtitle: string;
  settings: string;
  settingsTitle: string;
  settingsAria: string;
  toDarkMode: string;
  toLightMode: string;
  rescan: string;
  scanning: string;
  statsLabel: string;
  totalSize: (count: number) => string;
  activeLabel: (size: string) => string;
  archivedLabel: (size: string) => string;
  providerTotal: (name: string) => string;
  otherUsage: string;
  otherUsageLabel: string;
  searchPlaceholder: string;
  searchLabel: string;
  providerFilterLabel: string;
  statusFilterLabel: string;
  chipAll: (count: number) => string;
  chipActive: (count: number) => string;
  chipArchived: (count: number) => string;
  ageAny: string;
  age7: string;
  age30: string;
  age90: string;
  ageFilterLabel: string;
  sortSize: string;
  sortUpdated: string;
  sortLabel: string;
  toolbarLabel: string;
  selectAll: (count: number) => string;
  listLabel: string;
  emptyScanning: string;
  emptyNoMatch: string;
  badgeArchived: string;
  selectRow: (title: string) => string;
  hint: string;
  selectedSummary: (count: number, size: string) => string;
  clearSelected: (count: number) => string;
  archiveSelected: string;
  deleteSelected: string;
  confirmTitle: string;
  confirmBody: (count: number, size: string) => string;
  mode: string;
  trashOption: string;
  permanentOption: string;
  cancel: string;
  working: string;
  moveTrash: string;
  deletePermanently: string;
  defaultMode: string;
  trashRecommended: string;
  permanent: string;
  settingsNote: string;
  done: string;
  scanNotice: (count: number, size: string) => string;
  deletedNotice: (count: number, size: string) => string;
  deleteFailedSuffix: (count: number, message: string) => string;
  archivedNotice: (count: number) => string;
  archiveFailedSuffix: (count: number, message: string) => string;
}

const en: Strings = {
  reclaimStorage: "Reclaim database space",
  reclaimDescription: "Permanently remove event history for sessions that have already been deleted, then compact the database. Existing sessions and their events are preserved. This cannot be undone.",
  closeOpenCode: "Close OpenCode before continuing. Compaction may take a while and needs additional free disk space.",
  openCodeDeleteNote: "Includes the selected sessions’ messages and event history. The database will be compacted after deletion; actual disk space reclaimed may differ from the data size shown. Close OpenCode first.",
  sessionDataNote: "Includes messages and event history; data size is not the amount of disk space reclaimable.",
  reclaimedNotice: (count, size) => `Removed ${count} deleted-session event groups; reclaimed ${size} on disk`,
  compareUsage: "Agent usage",
  agentsCombined: (count) => `Across ${count} agents`,
  brandSubtitle: "Inspect and clean local coding-agent sessions",
  settings: "Settings",
  settingsTitle: "Settings",
  settingsAria: "Settings (⌘,)",
  toDarkMode: "Switch to dark mode",
  toLightMode: "Switch to light mode",
  rescan: "Rescan",
  scanning: "Scanning…",
  statsLabel: "Session storage stats",
  totalSize: (count) => `Total session size (${count})`,
  activeLabel: (size) => `Active · ${size}`,
  archivedLabel: (size) => `Archived · ${size}`,
  providerTotal: (name) => `${name} total`,
  otherUsage: "Other usage",
  otherUsageLabel: "Other disk usage (read-only)",
  searchPlaceholder: "Search title, directory, or session ID (⌘K)",
  searchLabel: "Search sessions",
  providerFilterLabel: "Select agent",
  statusFilterLabel: "Status filter",
  chipAll: (count) => `All ${count}`,
  chipActive: (count) => `Active ${count}`,
  chipArchived: (count) => `Archived ${count}`,
  ageAny: "Any time",
  age7: "Older than 7 days",
  age30: "Older than 30 days",
  age90: "Older than 90 days",
  ageFilterLabel: "Age filter",
  sortSize: "By size ↓",
  sortUpdated: "By updated ↓",
  sortLabel: "Sort order",
  toolbarLabel: "Filters and sorting",
  selectAll: (count) => `Select all (${count})`,
  listLabel: "Session list",
  emptyScanning: "Scanning agent session stores…",
  emptyNoMatch:
    "No sessions match the current filters. Adjust the filters or rescan.",
  badgeArchived: "Archived",
  selectRow: (title) => `Select ${title}`,
  hint: "Click a row to select it; use arrow keys to navigate, space or enter to toggle",
  selectedSummary: (count, size) => `${count} selected · ${size}`,
  clearSelected: (count) => `Clear (${count})`,
  archiveSelected: "Archive",
  deleteSelected: "Delete…",
  confirmTitle: "Delete sessions",
  confirmBody: (count, size) =>
    `About to process ${count} sessions totaling ${size}.`,
  mode: "Mode",
  trashOption: "Move to Trash (recoverable from Finder)",
  permanentOption: "Delete permanently (irrecoverable)",
  cancel: "Cancel",
  working: "Working…",
  moveTrash: "Move to Trash",
  deletePermanently: "Delete permanently",
  defaultMode: "Default mode",
  trashRecommended: "Move to Trash (recommended)",
  permanent: "Delete permanently",
  settingsNote:
    "You can still switch modes in the confirmation dialog before each delete. Stored locally.",
  done: "Done",
  scanNotice: (count, size) => `Scanned ${count} sessions, ${size} total`,
  deletedNotice: (count, size) => `Deleted ${count} sessions, freed ${size}`,
  deleteFailedSuffix: (count, message) => `; ${count} failed: ${message}`,
  archivedNotice: (count) => `Archived ${count} sessions`,
  archiveFailedSuffix: (count, message) => `; ${count} failed: ${message}`,
};

const zh: Strings = {
  reclaimStorage: "回收数据库空间",
  reclaimDescription: "永久清除已删除会话遗留的事件记录，然后压缩数据库。现有会话及其事件记录会保留。此操作不可撤销。",
  closeOpenCode: "请先关闭 OpenCode。压缩可能需要一些时间和额外的可用磁盘空间。",
  openCodeDeleteNote: "将删除所选会话的消息和事件记录，并压缩数据库。实际释放空间可能与显示的数据大小不同。请先关闭 OpenCode。",
  sessionDataNote: "包含消息和事件记录；数据大小不等于可释放的磁盘空间。",
  reclaimedNotice: (count, size) => `已清理 ${count} 个已删除会话的残留事件，实际回收磁盘空间 ${size}`,
  compareUsage: "Agent 用量对比",
  agentsCombined: (count) => `${count} 个 Agent 合计`,
  brandSubtitle: "统计与清理本机 Coding Agent 会话",
  settings: "设置",
  settingsTitle: "设置",
  settingsAria: "设置 (⌘,)",
  toDarkMode: "切换到深色模式",
  toLightMode: "切换到浅色模式",
  rescan: "重新扫描",
  scanning: "扫描中…",
  statsLabel: "会话占用统计",
  totalSize: (count) => `会话总大小（${count} 个）`,
  activeLabel: (size) => `活跃 · ${size}`,
  archivedLabel: (size) => `已归档 · ${size}`,
  providerTotal: (name) => `${name} 总占用`,
  otherUsage: "其他占用",
  otherUsageLabel: "其他目录占用（只读）",
  searchPlaceholder: "搜索标题、目录或会话 ID（⌘K）",
  searchLabel: "搜索会话",
  providerFilterLabel: "选择 Agent",
  statusFilterLabel: "状态筛选",
  chipAll: (count) => `全部 ${count}`,
  chipActive: (count) => `活跃 ${count}`,
  chipArchived: (count) => `归档 ${count}`,
  ageAny: "全部时间",
  age7: "7 天前",
  age30: "30 天前",
  age90: "90 天前",
  ageFilterLabel: "时间筛选",
  sortSize: "按大小 ↓",
  sortUpdated: "按更新时间 ↓",
  sortLabel: "排序方式",
  toolbarLabel: "筛选与排序",
  selectAll: (count) => `全选（${count}）`,
  listLabel: "会话列表",
  emptyScanning: "正在扫描 Agent 会话存储…",
  emptyNoMatch: "没有符合条件的会话。调整筛选条件或重新扫描。",
  badgeArchived: "归档",
  selectRow: (title) => `选择 ${title}`,
  hint: "点击行勾选会话；方向键导航，空格或回车切换勾选",
  selectedSummary: (count, size) => `已选 ${count} 个 · 共 ${size}`,
  clearSelected: (count) => `清除（${count}）`,
  archiveSelected: "归档",
  deleteSelected: "删除…",
  confirmTitle: "删除会话",
  confirmBody: (count, size) => `即将处理 ${count} 个会话，共 ${size}。`,
  mode: "处理方式",
  trashOption: "移到废纸篓（可从访达找回）",
  permanentOption: "永久删除（不可恢复）",
  cancel: "取消",
  working: "处理中…",
  moveTrash: "移到废纸篓",
  deletePermanently: "永久删除",
  defaultMode: "默认处理方式",
  trashRecommended: "移到废纸篓（推荐）",
  permanent: "永久删除",
  settingsNote:
    "每次删除前的确认对话框中仍可临时切换。设置保存在本机。",
  done: "完成",
  scanNotice: (count, size) => `已扫描 ${count} 个会话，共 ${size}`,
  deletedNotice: (count, size) => `已删除 ${count} 个会话，释放 ${size}`,
  deleteFailedSuffix: (count, message) => `；${count} 个失败：${message}`,
  archivedNotice: (count) => `已归档 ${count} 个会话`,
  archiveFailedSuffix: (count, message) => `；${count} 个失败：${message}`,
};

export const strings: Record<Locale, Strings> = { en, zh };

// Keep the legacy key so an in-place upgrade retains the user's locale.
const LOCALE_KEY = "codex-clear:locale";

export function loadLocale(): Locale {
  return localStorage.getItem(LOCALE_KEY) === "zh" ? "zh" : "en";
}

export function saveLocale(locale: Locale): void {
  localStorage.setItem(LOCALE_KEY, locale);
}

// The backend emits canonical English messages. Map the known prefixes to
// Chinese and pass anything unknown (or English locale) through unchanged.
const zhMessagePrefixes: [string, string][] = [
  [
    "Could not locate the user home directory",
    "无法定位用户主目录",
  ],
  ["Sessions directory not found:", "未找到会话目录："],
  [
    "No sessions/ or archived_sessions/ directory found",
    "未找到 sessions/ 或 archived_sessions/ 目录",
  ],
  ["Unknown session provider", "未知的会话 Provider"],
  ["Provider does not support archiving", "该 Provider 不支持归档"],
  ["Provider does not support moving sessions to trash", "该 Provider 不支持移到废纸篓"],
  ["Provider does not support permanent deletion", "该 Provider 不支持永久删除"],
  ["Invalid session path", "无效的会话路径"],
  [
    "Only rollout-*.jsonl session files can be deleted",
    "仅允许删除 rollout-*.jsonl 会话文件",
  ],
  ["File not found:", "文件不存在："],
  [
    "Target is not a regular file (symbolic link rejected)",
    "目标不是普通文件（已拒绝符号链接）",
  ],
  ["Failed to resolve path:", "无法解析路径："],
  [
    "Path is outside the Codex session directories",
    "路径不在 Codex 会话目录内",
  ],
  ["Failed to move to trash:", "移入废纸篓失败："],
  ["Failed to delete:", "删除失败："],
  ["Already archived", "已是归档会话"],
  [
    "An archived session with the same file name already exists",
    "已存在同名的归档会话文件",
  ],
  ["Failed to archive:", "归档失败："],
  [
    "Failed to create archived_sessions/:",
    "创建 archived_sessions/ 目录失败：",
  ],
];

export function localizeMessage(message: string, locale: Locale): string {
  if (locale === "en") return message;
  // Registry scan issues retain their provider name before the canonical message.
  const providerIssue = message.match(/^([^:]+): (.+)$/);
  if (providerIssue && zhMessagePrefixes.some(([prefix]) => providerIssue[2].startsWith(prefix))) {
    return `${providerIssue[1]}: ${localizeMessage(providerIssue[2], locale)}`;
  }
  for (const [prefix, translation] of zhMessagePrefixes) {
    if (message.startsWith(prefix)) {
      return translation + message.slice(prefix.length);
    }
  }
  return message;
}

const zhDirNames: Record<string, string> = {
  "Database overhead": "数据库索引、空闲页及其他数据",
  "Deleted-session events": "已删除会话的残留事件",
  Database: "数据库",
  Snapshots: "快照",
  "Tool output": "工具输出",
  Logs: "日志",
  Storage: "其他存储",
  "Log database": "日志数据库",
  Plugins: "插件",
  "Generated images": "生成图片",
  "Computer Use": "Computer Use",
  "Legacy database": "旧版数据库",
  Cache: "缓存",
};

export function localizeDirName(name: string, locale: Locale): string {
  if (locale === "en") return name;
  return zhDirNames[name] ?? name;
}

const zhTitles: Record<string, string> = {
  "Untitled session": "未命名会话",
};

export function localizeTitle(title: string, locale: Locale): string {
  if (locale === "en") return title;
  return zhTitles[title] ?? title;
}
