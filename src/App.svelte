<script lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { onMount } from "svelte";

type TimeSyncResult = { epoch_millis: number };

const HALF_SPEED = 0.5;
const NORMAL_SPEED = 1;
const THREE_HALVES_SPEED = 1.5;
const DOUBLE_SPEED = 2;
const TRIPLE_SPEED = 3;
const QUADRUPLE_SPEED = 4;
const TWO_THIRDS_SPEED = DOUBLE_SPEED / TRIPLE_SPEED;

const SPEED_OPTIONS = [
  { value: HALF_SPEED, label: "1/2×" },
  { value: TWO_THIRDS_SPEED, label: "2/3×" },
  { value: NORMAL_SPEED, label: "1×" },
  { value: THREE_HALVES_SPEED, label: "3/2×" },
  { value: DOUBLE_SPEED, label: "2×" },
  { value: TRIPLE_SPEED, label: "3×" },
  { value: QUADRUPLE_SPEED, label: "4×" },
];

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

let speed = NORMAL_SPEED;
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
let speedSelect: HTMLSelectElement | null = null;
let controlsOpen = false;
let controlsContainer: HTMLDivElement | null = null;
let toggleButtonEl: HTMLButtonElement | null = null;

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

function isValidSpeed(value: number): boolean {
  return SPEED_OPTIONS.some((option) => option.value === value);
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
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone;
  } catch {
    return "Etc/UTC";
  }
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
}

function handleSpeedSelection(event: Event) {
  const target = event.target as HTMLSelectElement;
  const value = Number(target.value);
  if (!(Number.isFinite(value) && isValidSpeed(value))) {
    return;
  }
  changeSpeed(value);
}

$: if (controlsOpen && speedSelect) {
  requestAnimationFrame(() => {
    speedSelect?.focus();
  });
}

function handleStagePointerDown(event: PointerEvent) {
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
  applySync(Date.now());
  requestSync(false);
  tick();
  schedulePeriodicSync();

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
    window.removeEventListener("keydown", handleEscape);
    window.removeEventListener("pointerdown", handleStagePointerDown, true);
  };
});
</script>

<div class="stage">
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
    >
      <div class="control-row">
        <span class="control-label">秒針速度</span>
        <select bind:this={speedSelect} on:change={handleSpeedSelection}>
          {#each SPEED_OPTIONS as option}
            <option value={option.value} selected={option.value === speed}>{option.label}</option>
          {/each}
        </select>
      </div>
      <button
        class="sync-button"
        type="button"
        on:click={() => requestSync(true)}
        disabled={syncing}
      >
        {syncing ? "同期中…" : "再同期"}
      </button>
    </div>
  {/if}

  <span class="sr-only" aria-live="polite">{uiState.statusMessage}</span>
</div>
