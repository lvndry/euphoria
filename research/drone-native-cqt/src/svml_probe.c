/* Arithmetic-only probe. No model inference, Python or NumPy dependencies. */
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <string.h>
#include <math.h>
extern int cqt_svml_log1p(const float*,float*,size_t);
static float *read_array(const char *path,size_t *count) {
    FILE *file=fopen(path,"rb");if(!file)return NULL;
    if(fseek(file,0,SEEK_END)) {fclose(file);return NULL;}
    long bytes=ftell(file);rewind(file);
    if(bytes<0 || bytes%4) {fclose(file);return NULL;}
    *count=(size_t)bytes/4;
    float *values=malloc((size_t)bytes);
    if(!values || fread(values,4,*count,file)!=*count) {fclose(file);free(values);return NULL;}
    fclose(file);return values;
}
int main(int argc,char **argv) {
    if(argc!=4)return 2;
    uint32_t endian=1;if(*(const unsigned char*)&endian!=1 || sizeof(float)!=4)return 3;
    size_t length=0,reference_length=0;
    float *input=read_array(argv[1],&length),*reference=read_array(argv[2],&reference_length);
    if(!input || !reference || length!=reference_length)return 4;
    float *output=calloc(length,sizeof(float));if(!output)return 5;
    int status=cqt_svml_log1p(input,output,length);if(status) {fprintf(stderr,"feature guard/status%d\n",status);return 6;}
    size_t differences=0;double maximum=0;
    for(size_t index=0;index<length;index++) {
        uint32_t actual_bits,expected_bits;
        memcpy(&actual_bits,output+index,4);memcpy(&expected_bits,reference+index,4);
        differences+=actual_bits!=expected_bits;
        double difference=fabs((double)output[index]-(double)reference[index]);if(difference>maximum)maximum=difference;
    }
    FILE *report=fopen(argv[3],"wx");if(!report)return 7;
    fprintf(report,"{\n  \"format\":\"k10-svml-arithmetic-probe-v1\",\n  \"values\":%zu,\n  \"bit_mismatches\":%zu,\n  \"max_abs\":%.17g,\n  \"native_dependencies\":\"vendored NumPy/SVML + glibc; no Python or NumPy\",\n  \"cpu_guard\":\"Linux x86-64 AVX512F/DQ/BW/VL and FMA required\"\n}\n",length,differences,maximum);
    fclose(report);free(input);free(reference);free(output);
    return differences?1:0;
}
