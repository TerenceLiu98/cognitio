import type { JobState, JobSummary, Locale } from "./types";

export const processingStates: JobState[] = [
  "preflight",
  "running",
  "verifying",
  "archiving",
  "cancelling",
];
export const activeStates: JobState[] = [
  "detected",
  "stabilizing",
  "queued",
  ...processingStates,
];

export function processingJobs(jobs: JobSummary[], limit = 3): JobSummary[] {
  return jobs
    .filter((job) => processingStates.includes(job.state))
    .slice(0, limit);
}

export function countJobs(jobs: JobSummary[], states: JobState[]): number {
  return jobs.filter((job) => states.includes(job.state)).length;
}

const phases: Record<JobSummary["stage"], [string, string]> = {
  queued: ["Waiting for processor", "等待处理"],
  preflight: ["Checking parser and agent", "检查解析器与 Agent"],
  parsing: ["Parsing PDF with MinerU", "使用 MinerU 解析 PDF"],
  waitingForWiki: ["Waiting for Wiki publication", "等待 Wiki 发布"],
  generating: ["Generating linked knowledge pages", "生成关联知识页面"],
  publishing: ["Verifying Git publication", "校验 Git 发布"],
  archiving: ["Archiving source PDF", "归档原始 PDF"],
  complete: ["Published", "已发布"],
};

export function jobPhase(job: JobSummary, locale: Locale): string {
  if (job.state === "cancelling")
    return locale === "zh" ? "等待执行停止" : "Waiting for execution to stop";
  if (
    job.state === "blocked" ||
    job.state === "failed" ||
    job.state === "cancelled"
  )
    return job.phase;
  return phases[job.stage]?.[locale === "zh" ? 1 : 0] ?? job.phase;
}
