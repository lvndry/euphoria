/* Research wrapper reproducing python-soxr 1.1.0 csoxr_split_ch<float>.
 * Wrapper source: BSD-3-Clause python-soxr; libsoxr: LGPL-2.1-or-later.
 * Input/output are mono FLOAT32 split buffers; block=96000, final NULL flush.
 */
#include <soxr.h>
#include <stddef.h>
int cqt_halve(const float *input, size_t length, float *output, size_t capacity, size_t *written) {
  soxr_error_t error = NULL;
  soxr_io_spec_t io = soxr_io_spec(SOXR_FLOAT32_S, SOXR_FLOAT32_S);
  soxr_quality_spec_t quality = soxr_quality_spec(SOXR_HQ, 0);
  soxr_t state = soxr_create(2., 1., 1, &error, &io, &quality, NULL);
  if (error || !state) return 1;
  size_t position = 0, done = 0;
  for (size_t start = 0; start < length; start += 96000) {
    const float *in = input + start;
    float *out = output + position;
    size_t count = length-start < 96000 ? length-start : 96000;
    error = soxr_process(state, &in, count, NULL, &out, capacity-position, &done);
    if (error) { soxr_delete(state); return 2; }
    position += done;
  }
  float *out = output + position;
  error = soxr_process(state, NULL, 0, NULL, &out, capacity-position, &done);
  position += done;
  soxr_delete(state);
  *written = position;
  return error ? 3 : 0;
}
