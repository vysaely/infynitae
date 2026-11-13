/*
 * Pure timing and statistics logic for frame-probe.
 * Split into a header so unit tests run cleanly without a Wayland display.
 */
#ifndef FRAME_PROBE_STATS_H
#define FRAME_PROBE_STATS_H

#include <inttypes.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#define FP_MAX_PENDING_MOTION 4096

struct fp_motion_item {
	int64_t evt_ns;
	uint64_t commit_seq;
};

struct fp_latency_summary {
	size_t count;
	double min_ms;
	double p50_ms;
	double p95_ms;
	double p99_ms;
	double max_ms;
	double mean_ms;
};

struct fp_stats {
	uint64_t frames_committed;
	uint64_t frames_presented;
	uint64_t frames_discarded;

	bool has_prev_present;
	int64_t prev_present_ns;
	uint64_t prev_seq;
	const void *prev_output;

	uint64_t seq_pairs;
	uint64_t seq_gaps;
	uint64_t seq_missed;

	uint64_t dt_pairs;
	uint64_t dt_gaps;
	uint64_t dt_missed;

	uint64_t output_switches;

	uint64_t motion_events;
	uint64_t motion_matched;
	uint64_t motion_clock_fallbacks;
	uint64_t motion_negative_discarded;
	uint64_t motion_dropped_overflow;

	/* Interval counters reset on each periodic printout. */
	uint64_t interval_frames;
	uint64_t interval_discarded;
	uint64_t interval_seq_pairs;
	uint64_t interval_seq_missed;
	uint64_t interval_dt_pairs;
	uint64_t interval_dt_missed;
	size_t interval_lat_n;
	int64_t interval_lat_min;
	int64_t interval_lat_max;
	int64_t interval_lat_sum;

	struct fp_motion_item pending[FP_MAX_PENDING_MOTION];
	size_t pending_head;
	size_t pending_count;

	int64_t *latencies_ns;
	size_t lat_len;
	size_t lat_cap;
};

static inline void fp_stats_reset_interval(struct fp_stats *s)
{
	s->interval_frames = 0;
	s->interval_discarded = 0;
	s->interval_seq_pairs = 0;
	s->interval_seq_missed = 0;
	s->interval_dt_pairs = 0;
	s->interval_dt_missed = 0;
	s->interval_lat_n = 0;
	s->interval_lat_min = 0;
	s->interval_lat_max = 0;
	s->interval_lat_sum = 0;
}

static inline void fp_stats_init(struct fp_stats *s)
{
	memset(s, 0, sizeof(*s));
}

static inline void fp_stats_free(struct fp_stats *s)
{
	free(s->latencies_ns);
	s->latencies_ns = NULL;
	s->lat_len = 0;
	s->lat_cap = 0;
}

static inline int fp_cmp_i64(const void *a, const void *b)
{
	int64_t x = *(const int64_t *)a;
	int64_t y = *(const int64_t *)b;
	return (x > y) - (x < y);
}

/*
 * Wayland wl_pointer.motion delivers a 32-bit wrapping millisecond timestamp.
 * Unwrap it relative to receipt time; fall back to recv_ns if outside a sane
 * window (different clock domain or stalled > 5 s).
 */
static inline int64_t fp_unwrap_motion_ns(uint32_t time_ms, int64_t recv_ns, bool *used_fallback)
{
	if (recv_ns < 0) {
		if (used_fallback)
			*used_fallback = true;
		return recv_ns;
	}
	int64_t recv_ms = recv_ns / 1000000LL;
	uint32_t recv_ms32 = (uint32_t)(uint64_t)recv_ms;
	int32_t diff_ms = (int32_t)(recv_ms32 - time_ms);
	if (diff_ms < -5 || diff_ms > 5000) {
		if (used_fallback)
			*used_fallback = true;
		return recv_ns;
	}
	if (used_fallback)
		*used_fallback = false;
	return (recv_ms - (int64_t)diff_ms) * 1000000LL;
}

static inline void fp_stats_record_motion(struct fp_stats *s, uint32_t time_ms, int64_t recv_ns)
{
	bool fallback = false;
	int64_t evt_ns = fp_unwrap_motion_ns(time_ms, recv_ns, &fallback);
	s->motion_events++;
	if (fallback)
		s->motion_clock_fallbacks++;

	if (s->pending_count == FP_MAX_PENDING_MOTION) {
		s->pending_head = (s->pending_head + 1) % FP_MAX_PENDING_MOTION;
		s->pending_count--;
		s->motion_dropped_overflow++;
	}
	size_t idx = (s->pending_head + s->pending_count) % FP_MAX_PENDING_MOTION;
	s->pending[idx].evt_ns = evt_ns;
	s->pending[idx].commit_seq = 0;
	s->pending_count++;
}

static inline void fp_stats_on_commit(struct fp_stats *s, uint64_t commit_seq)
{
	s->frames_committed++;
	for (size_t i = 0; i < s->pending_count; i++) {
		size_t idx = (s->pending_head + i) % FP_MAX_PENDING_MOTION;
		if (s->pending[idx].commit_seq == 0)
			s->pending[idx].commit_seq = commit_seq;
	}
}

static inline bool fp_stats_push_latency(struct fp_stats *s, int64_t lat_ns)
{
	if (s->lat_len == s->lat_cap) {
		size_t new_cap = s->lat_cap ? s->lat_cap * 2 : 256;
		int64_t *next = (int64_t *)realloc(s->latencies_ns, new_cap * sizeof(*next));
		if (!next)
			return false;
		s->latencies_ns = next;
		s->lat_cap = new_cap;
	}
	s->latencies_ns[s->lat_len++] = lat_ns;

	if (s->interval_lat_n == 0 || lat_ns < s->interval_lat_min)
		s->interval_lat_min = lat_ns;
	if (s->interval_lat_n == 0 || lat_ns > s->interval_lat_max)
		s->interval_lat_max = lat_ns;
	s->interval_lat_sum += lat_ns;
	s->interval_lat_n++;
	return true;
}

static inline void fp_stats_on_presented(struct fp_stats *s, uint64_t commit_seq,
					 int64_t present_ns, uint32_t refresh_ns,
					 uint64_t msc_seq, const void *sync_output)
{
	s->frames_presented++;
	s->interval_frames++;

	if (!s->has_prev_present) {
		s->has_prev_present = true;
		s->prev_present_ns = present_ns;
		s->prev_seq = msc_seq;
		s->prev_output = sync_output;
	} else if (sync_output != NULL && s->prev_output != NULL && sync_output != s->prev_output) {
		/* Moving across outputs shifts refresh cadence and MSC counter; reset baseline. */
		s->output_switches++;
		s->prev_present_ns = present_ns;
		s->prev_seq = msc_seq;
		s->prev_output = sync_output;
	} else {
		if (msc_seq != 0 && s->prev_seq != 0) {
			s->seq_pairs++;
			s->interval_seq_pairs++;
			if (msc_seq > s->prev_seq + 1) {
				uint64_t missed = (msc_seq - s->prev_seq) - 1;
				s->seq_gaps++;
				s->seq_missed += missed;
				s->interval_seq_missed += missed;
			}
		}
		if (refresh_ns > 0 && present_ns >= s->prev_present_ns) {
			int64_t dt = present_ns - s->prev_present_ns;
			int64_t half = (int64_t)refresh_ns / 2;
			s->dt_pairs++;
			s->interval_dt_pairs++;
			if (dt > (int64_t)refresh_ns + half) {
				uint64_t missed = (uint64_t)((dt + half) / (int64_t)refresh_ns) - 1;
				if (missed > 0) {
					s->dt_gaps++;
					s->dt_missed += missed;
					s->interval_dt_missed += missed;
				}
			}
		}
		s->prev_present_ns = present_ns;
		s->prev_seq = msc_seq;
		if (sync_output != NULL)
			s->prev_output = sync_output;
	}

	while (s->pending_count > 0) {
		struct fp_motion_item *item = &s->pending[s->pending_head];
		if (item->commit_seq == 0 || item->commit_seq > commit_seq)
			break;
		if (present_ns >= item->evt_ns) {
			if (fp_stats_push_latency(s, present_ns - item->evt_ns))
				s->motion_matched++;
		} else {
			s->motion_negative_discarded++;
		}
		s->pending_head = (s->pending_head + 1) % FP_MAX_PENDING_MOTION;
		s->pending_count--;
	}
}

static inline void fp_stats_on_discarded(struct fp_stats *s)
{
	s->frames_discarded++;
	s->interval_discarded++;
}

static inline struct fp_latency_summary fp_stats_latency_summary(const struct fp_stats *s)
{
	struct fp_latency_summary out = {0};
	if (s->lat_len == 0)
		return out;

	int64_t *sorted = (int64_t *)malloc(s->lat_len * sizeof(*sorted));
	if (!sorted)
		return out;
	memcpy(sorted, s->latencies_ns, s->lat_len * sizeof(*sorted));
	qsort(sorted, s->lat_len, sizeof(*sorted), fp_cmp_i64);

	size_t n = s->lat_len;
	double sum = 0.0;
	for (size_t i = 0; i < n; i++)
		sum += (double)sorted[i];

	out.count = n;
	out.min_ms = (double)sorted[0] / 1e6;
	out.p50_ms = (double)sorted[(size_t)(0.50 * (double)(n - 1) + 0.5)] / 1e6;
	out.p95_ms = (double)sorted[(size_t)(0.95 * (double)(n - 1) + 0.5)] / 1e6;
	out.p99_ms = (double)sorted[(size_t)(0.99 * (double)(n - 1) + 0.5)] / 1e6;
	out.max_ms = (double)sorted[n - 1] / 1e6;
	out.mean_ms = (sum / (double)n) / 1e6;
	free(sorted);
	return out;
}

#endif
