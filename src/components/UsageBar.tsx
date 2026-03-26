import type { UsageInfo } from "../types";

interface UsageBarProps {
  usage?: UsageInfo;
  loading?: boolean;
}

export function formatResetTime(
  resetAt: number | null | undefined,
  nowSeconds = Math.floor(Date.now() / 1000)
): string {
  if (!resetAt) return "";
  const diff = resetAt - nowSeconds;
  if (diff <= 0) return "now";
  if (diff < 60) return `${diff}s`;

  const totalMinutes = Math.floor(diff / 60);
  if (totalMinutes < 60) return `${totalMinutes}m`;

  const totalHours = Math.floor(totalMinutes / 60);
  const minutes = totalMinutes % 60;
  if (totalHours < 24) return `${totalHours}h ${minutes}m`;

  const days = Math.floor(totalHours / 24);
  const hours = totalHours % 24;
  return `${days}d ${hours}h ${minutes}m`;
}

function formatExactResetTime(resetAt: number | null | undefined): string {
  if (!resetAt) return "";

  const date = new Date(resetAt * 1000);
  const month = new Intl.DateTimeFormat(undefined, { month: "long" }).format(date);
  const day = date.getDate();
  const minutes = String(date.getMinutes()).padStart(2, "0");
  const period = date.getHours() >= 12 ? "PM" : "AM";
  const hour12 = date.getHours() % 12 || 12;

  return `${month} ${day}, ${hour12}:${minutes} ${period}`;
}

function formatWindowDuration(minutes: number | null | undefined): string {
  if (!minutes) return "";
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h`;
  return `${Math.floor(hours / 24)}d`;
}

function RateLimitBar({
  label,
  usedPercent,
  windowMinutes,
  resetsAt,
  loadingSkin = false,
}: {
  label: string;
  usedPercent: number;
  windowMinutes?: number | null;
  resetsAt?: number | null;
  loadingSkin?: boolean;
}) {
  const remainingPercent = Math.max(0, 100 - usedPercent);
  const colorClass =
    remainingPercent <= 10
      ? "theme-progress-fill--danger"
      : remainingPercent <= 30
        ? "theme-progress-fill--warn"
        : "theme-progress-fill--good";

  const windowLabel = formatWindowDuration(windowMinutes);
  const resetLabel = formatResetTime(resetsAt);
  const exactResetLabel = formatExactResetTime(resetsAt);
  const metaClass = loadingSkin
    ? "theme-loading-copy theme-loading-copy--muted"
    : "text-gray-500 dark:text-slate-400";
  const trackClass = loadingSkin
    ? "theme-progress-track theme-progress-track--loading"
    : "theme-progress-track";
  const fillClass = loadingSkin
    ? `${colorClass} theme-progress-fill--loading`
    : colorClass;

  return (
    <div className="space-y-1">
      <div className={`flex justify-between text-xs ${metaClass}`}>
        <span>
          {label} {windowLabel && `(${windowLabel})`}
        </span>
        <span>
          {remainingPercent.toFixed(0)}% left
          {resetLabel && ` - resets ${resetLabel}`}
          {resetLabel && exactResetLabel && ` (${exactResetLabel})`}
        </span>
      </div>
      <div className={`${trackClass} relative h-1.5 overflow-hidden rounded-full`}>
        <div
          className={`relative h-full transition-all duration-300 ${fillClass}`}
          style={{ width: `${Math.min(remainingPercent, 100)}%` }}
        />
      </div>
    </div>
  );
}

export function UsageBar({ usage, loading }: UsageBarProps) {
  if (loading && !usage) {
    return (
      <div className="space-y-2">
        <div className="animate-pulse text-xs italic text-gray-400 dark:text-slate-500">
          Fetching usage...
        </div>
        <div className="theme-progress-track h-1.5 animate-pulse overflow-hidden rounded-full">
          <div className="h-full w-2/3 theme-progress-fill--good opacity-50" />
        </div>
        <div className="theme-progress-track h-1.5 animate-pulse overflow-hidden rounded-full">
          <div className="h-full w-1/2 theme-progress-fill--good opacity-40" />
        </div>
      </div>
    );
  }

  if (!usage) {
    return (
      <div className="py-1 text-xs italic text-gray-400 dark:text-slate-500">
        Fetching usage...
      </div>
    );
  }

  if (usage.error) {
    return (
      <div className="py-1 text-xs italic text-gray-400 dark:text-slate-500">
        {usage.error}
      </div>
    );
  }

  const hasPrimary =
    usage.primary_used_percent !== null && usage.primary_used_percent !== undefined;
  const hasSecondary =
    usage.secondary_used_percent !== null &&
    usage.secondary_used_percent !== undefined;

  if (!hasPrimary && !hasSecondary) {
    return (
      <div className="py-1 text-xs italic text-gray-400 dark:text-slate-500">
        No rate limit data
      </div>
    );
  }

  const showRefreshingState = loading && !!usage && !usage.error;

  return (
    <div className="space-y-2">
      {showRefreshingState && (
        <div className="theme-loading-copy theme-loading-copy--muted text-xs italic">
          Refreshing usage...
        </div>
      )}
      {hasPrimary && (
        <RateLimitBar
          label="5h Limit"
          usedPercent={usage.primary_used_percent!}
          windowMinutes={usage.primary_window_minutes}
          resetsAt={usage.primary_resets_at}
          loadingSkin={showRefreshingState}
        />
      )}
      {hasSecondary && (
        <RateLimitBar
          label="Weekly Limit"
          usedPercent={usage.secondary_used_percent!}
          windowMinutes={usage.secondary_window_minutes}
          resetsAt={usage.secondary_resets_at}
          loadingSkin={showRefreshingState}
        />
      )}
      {usage.credits_balance && (
        <div
          className={`text-xs ${
            showRefreshingState
              ? "theme-loading-copy theme-loading-copy--muted"
              : "text-gray-500 dark:text-slate-400"
          }`}
        >
          Credits: {usage.credits_balance}
        </div>
      )}
    </div>
  );
}
