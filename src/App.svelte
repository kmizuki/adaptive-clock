<script lang="ts">
import { invoke } from "@tauri-apps/api/core";
import type { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { onMount } from "svelte";

type TimeSyncResult = { epoch_millis: number };

const HALF_SPEED = 0.5;
const NORMAL_SPEED = 1;
const THREE_HALVES_SPEED = 1.5;
const DOUBLE_SPEED = 2;
const TRIPLE_SPEED = 3;
const QUADRUPLE_SPEED = 4;
const THREE_QUARTERS_SPEED = 0.75;

const SPEED_OPTIONS = [
  { value: HALF_SPEED, label: "x0.5" },
  { value: THREE_QUARTERS_SPEED, label: "x0.75" },
  { value: NORMAL_SPEED, label: "x1" },
  { value: THREE_HALVES_SPEED, label: "x1.5" },
  { value: DOUBLE_SPEED, label: "x2" },
  { value: TRIPLE_SPEED, label: "x3" },
  { value: QUADRUPLE_SPEED, label: "x4" },
];

const SPEED_STORAGE_KEY = "adaptive-clock-speed";

function isValidSpeed(value: number): boolean {
  return SPEED_OPTIONS.some((option) => option.value === value);
}

const RESYNC_INTERVAL_MINUTES = 15;
const MILLISECONDS_PER_SECOND = 1000;
const SECONDS_PER_MINUTE = 60;
const MINUTES_PER_HOUR = 60;
const HOURS_PER_HALF_DAY = 12;
const SECONDS_PER_HOUR = MINUTES_PER_HOUR * SECONDS_PER_MINUTE;
const RESYNC_INTERVAL_MS =
  RESYNC_INTERVAL_MINUTES * SECONDS_PER_MINUTE * MILLISECONDS_PER_SECOND;
const FULL_ROTATION_DEGREES = 360;
const DEGREES_PER_SECOND = FULL_ROTATION_DEGREES / SECONDS_PER_MINUTE;
const DEGREES_PER_MINUTE = FULL_ROTATION_DEGREES / MINUTES_PER_HOUR;
const DEGREES_PER_HOUR = FULL_ROTATION_DEGREES / HOURS_PER_HALF_DAY;
const TIME_API_ENDPOINT = "https://timeapi.io/api/Time/current/zone?timeZone=";

const FALLBACK_LOCALE = "ja-JP";
const FALLBACK_TIME_ZONE = "Asia/Tokyo";

function resolveLocale(): string {
  if (typeof document !== "undefined") {
    const langAttr = document.documentElement.lang;
    if (typeof langAttr === "string" && langAttr) {
      return langAttr;
    }
  }

  if (typeof navigator === "undefined") {
    return FALLBACK_LOCALE;
  }

  const { languages, language } = navigator;
  if (Array.isArray(languages) && languages.length > 0) {
    return languages[0] ?? FALLBACK_LOCALE;
  }

  if (typeof language === "string" && language) {
    return language;
  }

  return FALLBACK_LOCALE;
}

const ACTIVE_LOCALE = resolveLocale();
const isWindowsPlatform =
  typeof navigator !== "undefined" &&
  /windows/i.test(
    navigator.userAgent ||
      (navigator as { userAgentData?: { platform?: string } }).userAgentData
        ?.platform ||
      ""
  );

let activeTimeZone = (() => {
  try {
    return (
      Intl.DateTimeFormat().resolvedOptions().timeZone ?? FALLBACK_TIME_ZONE
    );
  } catch {
    return FALLBACK_TIME_ZONE;
  }
})();

const DIGITAL_TIME_FORMATTER = new Intl.DateTimeFormat(ACTIVE_LOCALE, {
  hour: "2-digit",
  minute: "2-digit",
  hourCycle: "h23",
});

const DIGITAL_DATE_FORMATTER = new Intl.DateTimeFormat(ACTIVE_LOCALE, {
  weekday: "short",
  month: "short",
  day: "numeric",
});

const SYNC_TIME_FORMATTER = new Intl.DateTimeFormat(ACTIVE_LOCALE, {
  hour: "2-digit",
  minute: "2-digit",
  hourCycle: "h23",
});

type ThemeMode = "dark" | "light";
const THEME_STORAGE_KEY = "adaptive-clock-theme";
let theme: ThemeMode = "dark";

function setTheme(value: ThemeMode, persist = true) {
  theme = value;

  if (typeof document !== "undefined") {
    document.body.dataset.theme = value;
    document.documentElement.style.setProperty(
      "color-scheme",
      value === "light" ? "light" : "dark"
    );
  }

  if (persist && typeof localStorage !== "undefined") {
    try {
      localStorage.setItem(THEME_STORAGE_KEY, value);
    } catch {
      /* ignore storage failures */
    }
  }
}

function toggleTheme() {
  setTheme(theme === "dark" ? "light" : "dark");
}

async function ensureWindowPinning() {
  if (typeof window === "undefined") {
    return;
  }

  try {
    const windowHandle = await ensureWindowHandle();
    if (!windowHandle) {
      return;
    }
    const applyPinning = () => {
      windowHandle.setAlwaysOnBottom(false).catch(() => {
        /* ignore pinning error */
      });
      windowHandle.setAlwaysOnTop(true).catch(() => {
        /* ignore pinning error */
      });
      if (!isWindowsPlatform) {
        windowHandle.setVisibleOnAllWorkspaces(true).catch(() => {
          /* ignore pinning error */
        });
      }
    };

    applyPinning();

    if (!pinningInterval) {
      pinningInterval = window.setInterval(
        applyPinning,
        PINNING_REFRESH_INTERVAL_MS
      );
    }

    if (!reapplyPinning) {
      reapplyPinning = applyPinning;
      window.addEventListener("focus", reapplyPinning);
      window.addEventListener("blur", reapplyPinning);
      document.addEventListener("visibilitychange", reapplyPinning);
    }
  } catch {
    // Ignore failures; desktop app will still function without pinning.
  }
}

function getSavedSpeed(): number | null {
  if (typeof localStorage === "undefined") {
    return null;
  }

  try {
    const stored = localStorage.getItem(SPEED_STORAGE_KEY);
    if (stored) {
      const parsed = Number(stored);
      if (Number.isFinite(parsed) && isValidSpeed(parsed)) {
        return parsed;
      }
    }
  } catch {
    /* ignore storage failures */
  }

  return null;
}

let speed = getSavedSpeed() ?? NORMAL_SPEED;
const uiState = {
  hourAngle: 0,
  minuteAngle: 0,
  secondAngle: 0,
  statusMessage: "時刻同期を待機中…",
  digitalTime: DIGITAL_TIME_FORMATTER.format(new Date()),
  dateLabel: DIGITAL_DATE_FORMATTER.format(new Date()),
};

let lastSync: Date | null = null;
let syncing = false;
let syncError = "";

let baseSecondAngle = 0;
let syncedEpochMs = Date.now();
let syncedPerfMs = performance.now();
let animationFrame = 0;
let periodicSyncTimer: number | null = null;
let controlsOpen = false;
let controlsContainer: HTMLDivElement | null = null;
let toggleButtonEl: HTMLButtonElement | null = null;
let pinningInterval: number | null = null;
let reapplyPinning: (() => void) | null = null;
let cachedWindow: WebviewWindow | null = null;
let speedOptionsEl: HTMLDivElement | null = null;

const PINNING_REFRESH_INTERVAL_MS = 15_000;

function updateStatusMessage() {
  if (syncError) {
    uiState.statusMessage = `⚠️ ${syncError}`;
    return;
  }

  if (lastSync) {
    uiState.statusMessage = "";
    return;
  }

  uiState.statusMessage = "時刻同期を待機中…";
}

function currentTimeFromSync(nowPerf: number): Date {
  const elapsed = nowPerf - syncedPerfMs;
  return new Date(syncedEpochMs + elapsed);
}

function computeHourAngle(date: Date): number {
  const hours = date.getHours() % HOURS_PER_HALF_DAY;
  const minutes = date.getMinutes();
  const seconds = date.getSeconds();
  return (
    (hours + minutes / MINUTES_PER_HOUR + seconds / SECONDS_PER_HOUR) *
    DEGREES_PER_HOUR
  );
}

function computeSecondAngle(nowPerf: number): number {
  const elapsedSeconds = (nowPerf - syncedPerfMs) / MILLISECONDS_PER_SECOND;
  const angle = baseSecondAngle + elapsedSeconds * speed * DEGREES_PER_SECOND;
  const normalized = angle % FULL_ROTATION_DEGREES;
  return normalized >= 0 ? normalized : normalized + FULL_ROTATION_DEGREES;
}

function computeAlignedSecondAngle(current: Date, nextSpeed: number): number {
  const seconds =
    current.getSeconds() + current.getMilliseconds() / MILLISECONDS_PER_SECOND;
  const secondsUntilNextMinute = SECONDS_PER_MINUTE - seconds;
  const travelSeconds =
    (nextSpeed * secondsUntilNextMinute) % SECONDS_PER_MINUTE;
  const targetSeconds =
    (SECONDS_PER_MINUTE - travelSeconds) % SECONDS_PER_MINUTE;
  return (targetSeconds / SECONDS_PER_MINUTE) * FULL_ROTATION_DEGREES;
}

function refreshHands(nowPerf: number) {
  const current = currentTimeFromSync(nowPerf);
  uiState.minuteAngle =
    (current.getMinutes() + current.getSeconds() / SECONDS_PER_MINUTE) *
    DEGREES_PER_MINUTE;
  uiState.hourAngle = computeHourAngle(current);
  uiState.secondAngle = computeSecondAngle(nowPerf);
  uiState.digitalTime = DIGITAL_TIME_FORMATTER.format(current);
  uiState.dateLabel = DIGITAL_DATE_FORMATTER.format(current);
}

function applySync(epochMillis: number) {
  syncedEpochMs = epochMillis;
  syncedPerfMs = performance.now();
  const syncedDate = new Date(epochMillis);
  baseSecondAngle = computeAlignedSecondAngle(syncedDate, speed);
  refreshHands(syncedPerfMs);
}

function getLocalTimeZone(): string {
  return activeTimeZone;
}

function supportsTauriInvoke(): boolean {
  if (typeof window === "undefined") {
    return false;
  }

  const internals = (
    window as {
      __TAURI_INTERNALS__?: {
        invoke?: unknown;
      };
    }
  ).__TAURI_INTERNALS__;

  return typeof internals?.invoke === "function";
}

async function ensureWindowHandle(): Promise<WebviewWindow | null> {
  if (!supportsTauriInvoke()) {
    return null;
  }

  if (cachedWindow) {
    return cachedWindow;
  }

  try {
    const { WebviewWindow: WebviewWindowModule } = await import(
      "@tauri-apps/api/webviewWindow"
    );
    cachedWindow = WebviewWindowModule.getCurrent();
    return cachedWindow;
  } catch {
    return null;
  }
}

async function fetchRemoteTime(timeZone: string): Promise<TimeSyncResult> {
  const response = await fetch(
    `${TIME_API_ENDPOINT}${encodeURIComponent(timeZone)}`,
    {
      method: "GET",
      headers: {
        Accept: "application/json",
      },
    }
  );

  if (!response.ok) {
    throw new Error(`Time API responded with status ${response.status}`);
  }

  const payload = (await response.json()) as {
    unixTime?: number;
    dateTime?: string;
    dateTimeUtc?: string;
    currentLocalTime?: string;
    currentUtcTime?: string;
    year?: number;
    month?: number;
    day?: number;
    hour?: number;
    minute?: number;
    seconds?: number;
    milliSeconds?: number;
  };

  const directUnix = payload.unixTime;
  if (typeof directUnix === "number" && Number.isFinite(directUnix)) {
    return { epoch_millis: directUnix * MILLISECONDS_PER_SECOND };
  }

  const isoCandidates = [
    payload.dateTime,
    payload.dateTimeUtc,
    payload.currentLocalTime,
    payload.currentUtcTime,
  ];

  for (const candidate of isoCandidates) {
    if (typeof candidate === "string") {
      const parsed = Date.parse(candidate);
      if (Number.isFinite(parsed)) {
        return { epoch_millis: parsed };
      }
    }
  }

  if (
    typeof payload.year === "number" &&
    typeof payload.month === "number" &&
    typeof payload.day === "number" &&
    typeof payload.hour === "number" &&
    typeof payload.minute === "number" &&
    typeof payload.seconds === "number"
  ) {
    const milliseconds =
      typeof payload.milliSeconds === "number" ? payload.milliSeconds : 0;
    const timestamp = Date.UTC(
      payload.year,
      payload.month - 1,
      payload.day,
      payload.hour,
      payload.minute,
      payload.seconds,
      milliseconds
    );
    if (Number.isFinite(timestamp)) {
      return { epoch_millis: timestamp };
    }
  }

  throw new Error("Time API returned an unexpected payload");
}

async function requestSync(manual = false) {
  if (syncing && !manual) {
    return;
  }

  syncing = true;
  syncError = "";
  updateStatusMessage();

  try {
    const timeZone = getLocalTimeZone();
    const result = supportsTauriInvoke()
      ? ((await invoke("sync_time", {
          time_zone: timeZone,
        })) as TimeSyncResult)
      : await fetchRemoteTime(timeZone);
    applySync(result.epoch_millis);
    lastSync = new Date();
    updateStatusMessage();
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    syncError = manual ? `手動同期に失敗しました: ${message}` : message;
    updateStatusMessage();
  } finally {
    syncing = false;
  }
}

function changeSpeed(next: number) {
  const nowPerf = performance.now();
  const current = currentTimeFromSync(nowPerf);
  baseSecondAngle = computeAlignedSecondAngle(current, next);
  const elapsedSinceSync = nowPerf - syncedPerfMs;
  if (Number.isFinite(elapsedSinceSync)) {
    syncedEpochMs += elapsedSinceSync;
  }
  syncedPerfMs = nowPerf;
  speed = next;
  refreshHands(nowPerf);

  if (typeof localStorage !== "undefined") {
    try {
      localStorage.setItem(SPEED_STORAGE_KEY, String(next));
    } catch {
      /* ignore storage failures */
    }
  }
}

function handleStagePointerDown(event: PointerEvent) {
  if (speedOptionsEl && !event.composedPath().includes(speedOptionsEl)) {
    // Ensure radio focus ring disappears when clicking elsewhere inside the HUD.
    speedOptionsEl
      .querySelector<HTMLInputElement>('input[name="speed"]:focus')
      ?.blur();
  }

  if (!controlsOpen) {
    return;
  }

  const path = event.composedPath();
  if (toggleButtonEl && path.includes(toggleButtonEl)) {
    return;
  }

  if (controlsContainer && path.includes(controlsContainer)) {
    return;
  }

  controlsOpen = false;
}

$: if (controlsOpen) {
  requestAnimationFrame(() => {
    speedOptionsEl
      ?.querySelector<HTMLInputElement>('input[name="speed"]:checked')
      ?.focus();
  });
}

async function closeWindow() {
  if (typeof window === "undefined") {
    return;
  }

  const windowHandle = await ensureWindowHandle();
  if (!windowHandle) {
    return;
  }

  try {
    await windowHandle.close();
  } catch {
    // Closing is not available in this environment.
  }
}

function tick() {
  const nowPerf = performance.now();
  refreshHands(nowPerf);
  animationFrame = requestAnimationFrame(tick);
}

function schedulePeriodicSync() {
  if (periodicSyncTimer) {
    clearInterval(periodicSyncTimer);
  }

  periodicSyncTimer = window.setInterval(() => {
    requestSync(false);
  }, RESYNC_INTERVAL_MS);
}

onMount(() => {
  let initialTheme: ThemeMode = "dark";
  if (typeof localStorage !== "undefined") {
    const storedTheme = localStorage.getItem(THEME_STORAGE_KEY);
    if (storedTheme === "light" || storedTheme === "dark") {
      initialTheme = storedTheme;
    }
  } else if (typeof window !== "undefined") {
    const prefersLight = window.matchMedia?.("(prefers-color-scheme: light)");
    if (prefersLight?.matches) {
      initialTheme = "light";
    }
  }

  setTheme(initialTheme, false);

  applySync(Date.now());
  requestSync(false);
  tick();
  schedulePeriodicSync();
  ensureWindowPinning();

  const handleEscape = (event: KeyboardEvent) => {
    if (event.key === "Escape") {
      controlsOpen = false;
    }
  };

  window.addEventListener("keydown", handleEscape);
  window.addEventListener("pointerdown", handleStagePointerDown, {
    capture: true,
  });

  return () => {
    cancelAnimationFrame(animationFrame);
    if (periodicSyncTimer) {
      clearInterval(periodicSyncTimer);
      periodicSyncTimer = null;
    }
    if (pinningInterval) {
      clearInterval(pinningInterval);
      pinningInterval = null;
    }
    if (reapplyPinning) {
      window.removeEventListener("focus", reapplyPinning);
      window.removeEventListener("blur", reapplyPinning);
      document.removeEventListener("visibilitychange", reapplyPinning);
      reapplyPinning = null;
    }
    window.removeEventListener("keydown", handleEscape);
    window.removeEventListener("pointerdown", handleStagePointerDown, true);

    if (typeof document !== "undefined") {
      delete document.body.dataset.theme;
      document.documentElement.style.removeProperty("color-scheme");
    }
  };
});
</script>

<div class="stage" data-tauri-drag-region="true">
  <button
    class="window-close"
    type="button"
    aria-label="アプリを閉じる"
    data-tauri-drag-region="false"
    on:click={closeWindow}
  >
    <span class="window-close-icon" aria-hidden="true"></span>
  </button>
  <div class="clock">
    <div class="dial">
      <div class="hand hour" style={`--angle: ${uiState.hourAngle}deg;`}></div>
      <div class="hand minute" style={`--angle: ${uiState.minuteAngle}deg;`}></div>
      <div class="hand second" style={`--angle: ${uiState.secondAngle}deg;`}></div>
      <div class="center-cap"></div>
    </div>
    <div class="info-layer" aria-hidden="true">
      <div class="digital-time">{uiState.digitalTime}</div>
      <div class="digital-date">{uiState.dateLabel}</div>
      <div class="status-hint">{uiState.statusMessage}</div>
    </div>
  </div>

  <button
    class="controls-toggle"
    type="button"
    aria-expanded={controlsOpen}
    aria-controls="control-panel"
    bind:this={toggleButtonEl}
    data-tauri-drag-region="false"
    on:click={() => {
      controlsOpen = !controlsOpen;
    }}
    on:keydown={(event) => {
      if (event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        controlsOpen = !controlsOpen;
      }
    }}
  >
    <span class="sr-only">設定を{controlsOpen ? "閉じる" : "開く"}</span>
    <svg viewBox="0 0 24 24" role="presentation" aria-hidden="true">
      <path
        d="M12 15.5a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7Zm8.35-2.66-1.34-.77a6.04 6.04 0 0 0 0-1.14l1.34-.77a.69.69 0 0 0 .32-.82 9.93 9.93 0 0 0-1.63-2.82.69.69 0 0 0-.84-.18l-1.33.77a6.25 6.25 0 0 0-.99-.58l-.2-1.56a.69.69 0 0 0-.57-.6 9.92 9.92 0 0 0-3.27 0 .69.69 0 0 0-.57.6l-.2 1.56a6.25 6.25 0 0 0-.99.58l-1.33-.77a.69.69 0 0 0-.84.18 9.95 9.95 0 0 0-1.63 2.82.69.69 0 0 0 .32.82l1.34.77a5.8 5.8 0 0 0 0 1.14l-1.34.77a.69.69 0 0 0-.32.82c.36 1.05.91 2.02 1.63 2.82a.69.69 0 0 0 .84.18l1.33-.77c.3.22.64.42.99.58l.2 1.56c.04.29.26.53.57.6a9.92 9.92 0 0 0 3.27 0 .69.69 0 0 0 .57-.6l.2-1.56c.35-.16.69-.36.99-.58l1.33.77a.69.69 0 0 0 .84-.18c.72-.8 1.27-1.77 1.63-2.82a.69.69 0 0 0-.32-.82Z"
      />
    </svg>
  </button>

  {#if controlsOpen}
    <div
      id="control-panel"
      class="hud"
      role="group"
      aria-label="時計の設定"
      bind:this={controlsContainer}
      data-tauri-drag-region="false"
    >
      <button
        class="theme-toggle"
        type="button"
        data-tauri-drag-region="false"
        on:click={toggleTheme}
      >
        <span class="theme-toggle-icon" aria-hidden="true">
          {#if theme === "dark"}
            <svg viewBox="0 0 24 24" focusable="false">
              <path
                d="M21 12.79A9 9 0 0 1 11.21 3 7 7 0 1 0 21 12.79Z"
              />
            </svg>
          {:else}
            <svg viewBox="0 0 24 24" focusable="false">
              <path
                d="M12 4.75a.75.75 0 0 1-.75-.75V2a.75.75 0 0 1 1.5 0v2a.75.75 0 0 1-.75.75Zm0 14.5a.75.75 0 0 1 .75.75v2a.75.75 0 0 1-1.5 0v-2a.75.75 0 0 1 .75-.75Zm8.25-7.75H22a.75.75 0 0 1 0 1.5h-1.75a.75.75 0 0 1 0-1.5ZM3.75 12A.75.75 0 0 1 3 12.75H1.25a.75.75 0 0 1 0-1.5H3a.75.75 0 0 1 .75.75ZM18.6 6.1a.75.75 0 0 1 0-1.06l1.23-1.24a.75.75 0 0 1 1.06 1.06l-1.23 1.24a.75.75 0 0 1-1.06 0ZM5.4 17.9a.75.75 0 0 1 0 1.06l-1.23 1.24a.75.75 0 0 1-1.06-1.06l1.23-1.24a.75.75 0 0 1 1.06 0Zm12.2 1.06a.75.75 0 0 1 1.06 0l1.23 1.24a.75.75 0 0 1-1.06 1.06l-1.23-1.24a.75.75 0 0 1 0-1.06ZM6.1 5.4a.75.75 0 0 1-1.06 0L3.81 4.16A.75.75 0 1 1 4.87 3.1L6.1 4.34a.75.75 0 0 1 0 1.06ZM12 7.5a4.5 4.5 0 1 0 0 9 4.5 4.5 0 0 0 0-9Z"
              />
            </svg>
          {/if}
        </span>
        <span class="theme-toggle-label">
          {theme === "dark" ? "ダークテーマ" : "ライトテーマ"}
        </span>
      </button>
      <fieldset class="speed-group" data-tauri-drag-region="false">
        <legend class="control-label">秒針速度</legend>
        <div class="speed-options" bind:this={speedOptionsEl}>
          {#each SPEED_OPTIONS as option}
            <label class="speed-option">
              <input
                type="radio"
                name="speed"
                value={option.value}
                checked={option.value === speed}
                on:change={() => changeSpeed(option.value)}
              />
              <span>{option.label}</span>
            </label>
          {/each}
        </div>
      </fieldset>
      <button
        class="sync-button"
        type="button"
        on:click={() => requestSync(true)}
        disabled={syncing}
        data-tauri-drag-region="false"
      >
        {syncing ? "同期中…" : "再同期"}
      </button>
    </div>
  {/if}

  <span class="sr-only" aria-live="polite">{uiState.statusMessage}</span>
</div>
