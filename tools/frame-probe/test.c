#include "probe_stats.h"

#include <assert.h>
#include <math.h>
#include <stdio.h>

static void test_unwrap_wraparound_and_fallback(void)
{
	bool fallback = false;

	/* Just before 2^32 ms rollover -> receipt just after rollover (10 ms wrap). */
	int64_t wrap_base_ms = (1LL << 32);
	int64_t recv_ns = (wrap_base_ms + 10) * 1000000LL + 350000LL;
	uint32_t evt_ms = (uint32_t)(wrap_base_ms - 6); /* 16 ms older */
	int64_t unwrapped = fp_unwrap_motion_ns(evt_ms, recv_ns, &fallback);
	assert(!fallback);
	assert(unwrapped == (wrap_base_ms - 6) * 1000000LL);

	/* Small negative delta (-1 ms) due to rounding / close clock ticks stays valid. */
	unwrapped = fp_unwrap_motion_ns(1001, 1000500000LL, &fallback);
	assert(!fallback);
	assert(unwrapped == 1001000000LL);

	/* Synthetic 0 ms timestamp far from monotonic recv_ns falls back to recv_ns. */
	unwrapped = fp_unwrap_motion_ns(0, 60000LL * 1000000LL, &fallback);
	assert(fallback);
	assert(unwrapped == 60000LL * 1000000LL);
}

static void test_missed_refreshes_seq_and_zero_seq_fallback(void)
{
	struct fp_stats s;
	fp_stats_init(&s);

	const uint32_t refresh = 16666667U;
	const void *out_a = (const void *)0x1;
	const void *out_b = (const void *)0x2;

	/* Case A: compositor supplies non-zero MSC sequence numbers. */
	fp_stats_on_presented(&s, 1, 1000000000LL, refresh, 100, out_a);
	fp_stats_on_presented(&s, 2, 1016666667LL, refresh, 101, out_a);
	fp_stats_on_presented(&s, 3, 1066666668LL, refresh, 104, out_a); /* skipped 2 refreshes */
	fp_stats_on_presented(&s, 4, 1083333335LL, refresh, 105, out_a);

	assert(s.seq_pairs == 3);
	assert(s.seq_gaps == 1);
	assert(s.seq_missed == 2);
	assert(s.dt_pairs == 3);
	assert(s.dt_gaps == 1);
	assert(s.dt_missed == 2);

	/* Moving across outputs resets baseline without false gap. */
	fp_stats_on_presented(&s, 5, 1183333335LL, refresh, 5, out_b);
	assert(s.output_switches == 1);
	assert(s.seq_gaps == 1);
	assert(s.seq_missed == 2);
	fp_stats_free(&s);

	/* Case B: compositor passes seq == 0 (time-based fallback). */
	fp_stats_init(&s);
	fp_stats_on_presented(&s, 1, 1000000000LL, refresh, 0, out_a);
	fp_stats_on_presented(&s, 2, 1020000000LL, refresh, 0, out_a); /* +20 ms < 25 ms -> ok */
	fp_stats_on_presented(&s, 3, 1053333334LL, refresh, 0, out_a); /* +33.3 ms -> missed 1 */
	fp_stats_on_presented(&s, 4, 1103333335LL, refresh, 0, out_a); /* +50.0 ms -> missed 2 */

	assert(s.seq_pairs == 0);
	assert(s.seq_missed == 0);
	assert(s.dt_pairs == 3);
	assert(s.dt_gaps == 2);
	assert(s.dt_missed == 3);
	fp_stats_free(&s);
}

static void test_motion_pairing_with_discarded_frame(void)
{
	struct fp_stats s;
	fp_stats_init(&s);

	/* Frame 1 has two motions; then frame 1 is discarded before presentation. */
	fp_stats_record_motion(&s, 1000, 1000200000LL); /* evt = 1000 ms */
	fp_stats_record_motion(&s, 1004, 1004200000LL); /* evt = 1004 ms */
	fp_stats_on_commit(&s, 1);

	/* Frame 2 commits another motion before feedback arrives, and frame 1 is discarded. */
	fp_stats_record_motion(&s, 1009, 1009200000LL); /* evt = 1009 ms */
	fp_stats_on_commit(&s, 2);
	fp_stats_on_discarded(&s);

	/* Motion arriving AFTER commit 2 must wait for commit 3. */
	fp_stats_record_motion(&s, 1015, 1015200000LL); /* evt = 1015 ms */

	/* Frame 2 is presented at 1020 ms: pairs the three committed motions (20, 16, 11 ms). */
	fp_stats_on_presented(&s, 2, 1020000000LL, 16666667U, 1, (const void *)0x1);
	assert(s.frames_discarded == 1);
	assert(s.frames_presented == 1);
	assert(s.motion_events == 4);
	assert(s.motion_matched == 3);
	assert(s.pending_count == 1);

	/* Frame 3 commits and presents the 4th motion at 1036 ms (latency = 21 ms). */
	fp_stats_on_commit(&s, 3);
	fp_stats_on_presented(&s, 3, 1036000000LL, 16666667U, 2, (const void *)0x1);
	assert(s.motion_matched == 4);
	assert(s.pending_count == 0);

	struct fp_latency_summary sum = fp_stats_latency_summary(&s);
	assert(sum.count == 4);
	assert(fabs(sum.min_ms - 11.0) < 1e-6);
	assert(fabs(sum.max_ms - 21.0) < 1e-6);
	/* Sorted: [11, 16, 20, 21] -> nearest rank idx(0.5 * 3 + 0.5)=2 => 20, mean => 17.0 */
	assert(fabs(sum.p50_ms - 20.0) < 1e-6);
	assert(fabs(sum.mean_ms - 17.0) < 1e-6);

	fp_stats_free(&s);
}

static void test_overflow_and_negative_rejection(void)
{
	struct fp_stats s;
	fp_stats_init(&s);

	/* Push more than FP_MAX_PENDING_MOTION events; oldest entries drop cleanly. */
	for (int i = 0; i < FP_MAX_PENDING_MOTION + 5; i++)
		fp_stats_record_motion(&s, (uint32_t)(1000 + i), (int64_t)(1000 + i) * 1000000LL);
	assert(s.motion_events == FP_MAX_PENDING_MOTION + 5);
	assert(s.motion_dropped_overflow == 5);
	assert(s.pending_count == FP_MAX_PENDING_MOTION);

	fp_stats_on_commit(&s, 1);
	/* Present timestamp earlier than events must reject cleanly rather than underflowing. */
	fp_stats_on_presented(&s, 1, 500LL * 1000000LL, 16666667U, 1, (const void *)0x1);
	assert(s.motion_matched == 0);
	assert(s.motion_negative_discarded == FP_MAX_PENDING_MOTION);
	assert(s.pending_count == 0);

	fp_stats_free(&s);
}

int main(void)
{
	test_unwrap_wraparound_and_fallback();
	test_missed_refreshes_seq_and_zero_seq_fallback();
	test_motion_pairing_with_discarded_frame();
	test_overflow_and_negative_rejection();
	printf("frame-probe stats tests passed (4/4)\n");
	return 0;
}
