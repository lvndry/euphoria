/* Research-only Linux x86-64 compatibility wrapper for the pinned BSD upstream
 * NumPy/SVML routine. No Python/NumPy linkage; unsupported CPUs fail explicitly.
 */
#include <stddef.h>
#include <math.h>
#if defined(__linux__) && defined(__x86_64__)
#include <immintrin.h>
extern __m512 __svml_log1pf16(__m512);
__attribute__((target("avx512f,avx512dq,avx512bw,avx512vl,fma")))
static void apply(const float *input, float *output, size_t length) {
    size_t index=0;
    for (;index+16<=length;index+=16) {
        __m512 values=_mm512_loadu_ps(input+index);
        _mm512_storeu_ps(output+index,__svml_log1pf16(values));
    }
    if (index<length) {
        float source[16]={0};float target[16];
        for(size_t lane=0;lane<length-index;lane++) source[lane]=input[index+lane];
        _mm512_storeu_ps(target,__svml_log1pf16(_mm512_loadu_ps(source)));
        for(size_t lane=0;lane<length-index;lane++) output[index+lane]=target[lane];
    }
}
#endif
int cqt_svml_log1p(const float *input,float *output,size_t length) {
#if defined(__linux__) && defined(__x86_64__)
    __builtin_cpu_init();
    if (!__builtin_cpu_supports("avx512f") || !__builtin_cpu_supports("avx512dq") || !__builtin_cpu_supports("avx512bw") || !__builtin_cpu_supports("avx512vl") || !__builtin_cpu_supports("fma")) return -1;
    for(size_t index=0;index<length;index++) if(!isfinite(input[index]) || input[index]<0) return -2;
    apply(input,output,length);
    return 0;
#else
    (void)input;(void)output;(void)length;
    return -3;
#endif
}
