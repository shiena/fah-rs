/* Minimal C host for one RGBA image or a concatenated RGBA video stream. */
#include "fah_engine.h"
#include <stdio.h>
#include <stdlib.h>

static int failed(int status) {
    if (status == FAH_OK) return 0;
    char error[2048];
    fah_engine_last_error(error, sizeof error);
    fprintf(stderr, "%s\n", error);
    return 1;
}
int main(int argc, char **argv) {
    if (argc < 6) {
        fprintf(stderr,"Usage: engine-check MODELS RGBA WIDTH HEIGHT BACKEND [NATIVE_LIBRARY]\n");
        return 2;
    }
    uint32_t width=(uint32_t)strtoul(argv[3],NULL,10);
    uint32_t height=(uint32_t)strtoul(argv[4],NULL,10);
    if(!width || width>8192 || !height || height>8192) return 2;
    size_t frame_size=(size_t)width*height*4;
    unsigned char *pixels=(unsigned char*)malloc(frame_size);
    if(!pixels) return 3;
    FILE *file=fopen(argv[2],"rb");
    if(!file) { free(pixels); return 3; }
    FahConfig config={FAH_ENGINE_ABI,sizeof(FahConfig),argv[1],argc>6?argv[6]:NULL,(uint32_t)atoi(argv[5])};
    FahHandle session=0,result=0;
    uint64_t frames=0;
    int code=1;
    if(failed(fah_engine_create(&config,&session))) goto cleanup;
    for (;;) {
        size_t count=fread(pixels,1,frame_size,file);
        if(!count && feof(file)) break;
        if(count!=frame_size) { fprintf(stderr,"Incomplete RGBA frame\n"); goto cleanup; }
        FahImage image={pixels,frame_size,width,height,(size_t)width*4,frames==0};
        if(failed(fah_engine_process(session,&image,&result))) goto cleanup;
        FahResult data;
        if(failed(fah_engine_result_data(result,&data,sizeof data))) goto cleanup;
        if(data.abi_version!=FAH_ENGINE_ABI || data.hand_count>2) goto cleanup;
        size_t length=0;
        if(fah_engine_result_json(result,NULL,0,&length)!=FAH_BUFFER_TOO_SMALL) goto cleanup;
        char *json=(char*)malloc(length);
        if(!json) goto cleanup;
        int status=fah_engine_result_json(result,json,length,&length);
        if(failed(status)) { free(json); goto cleanup; }
        puts(json);
        free(json);
        if(failed(fah_engine_result_destroy(result))) goto cleanup;
        result=0;
        ++frames;
    }
    code=frames ? 0 : 1;
cleanup:
    free(pixels);
    fclose(file);
    if(result && failed(fah_engine_result_destroy(result))) code=1;
    if(session && failed(fah_engine_destroy(session))) code=1;
    if(failed(fah_engine_shutdown())) code=1;
    return code;
}
