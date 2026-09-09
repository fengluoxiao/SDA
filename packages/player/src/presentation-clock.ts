/** Display-only extrapolation between native cursor reports. Never used for PCM pacing. */
export class PresentationClock {
  private confirmed = -1;
  private reportedAt = 0;
  private presented = 0;
  private running = false;

  reset(): void {
    this.confirmed = -1;
    this.reportedAt = 0;
    this.presented = 0;
    this.running = false;
  }

  read(confirmed: number, now: number, sampleRate: number, running: boolean): number {
    if (confirmed > this.confirmed || running !== this.running) {
      this.confirmed = Math.max(this.confirmed, confirmed);
      this.reportedAt = now;
    }
    this.running = running;
    // Stop predicting after one normal health-report interval when IPC or
    // playback stalls. A late report must not pull the image backwards.
    const elapsed = running ? Math.min(100, Math.max(0, now - this.reportedAt)) : 0;
    this.presented = Math.max(this.presented, this.confirmed + elapsed * sampleRate / 1000);
    return this.presented;
  }
}
