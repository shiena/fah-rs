/* Minimal C host for the same ABI used by Unity and Godot. */
#include "fah_engine.h"
#include <stdio.h>
#include <stdlib.h>
static void check(int status) {
    if (status != FAH_OK) { char error[2048]; fah_engine_last_error(error, sizeof error); fprintf(stderr, "%s\n", error); exit(1); }
}
int main(int argc, char **argv) {
    if (argc < 6) { fprintf(stderr,"Usage: engine-check MODELS RGBA WIDTH HEIGHT BACKEND [NATIVE_LIBRARY]\n"); return 2; }
    FILE *file=fopen(argv[2],"rb"); if(!file) return 3;
    fseek(file,0,SEEK_END); long size=ftell(file); rewind(file);
    if(size<=0) return 4;
    unsigned char *pixels=(unsigned char*)malloc((size_t)size); if(!pixels) return 5;
    if(fread(pixels,1,(size_t)size,file)!=(size_t)size) return 6;
    fclose(file);
    FahConfig config={FAH_ENGINE_ABI,sizeof(FahConfig),argv[1],argc>6?argv[6]:NULL,(uint32_t)atoi(argv[5])};
    FahHandle session=0,result=0;
    check(fah_engine_create(&config,&session));
    FahImage image={pixels,(size_t)size,(uint32_t)atoi(argv[3]),(uint32_t)atoi(argv[4]),(size_t)atoi(argv[3])*4,1};
    check(fah_engine_process(session,&image,&result));
    free(pixels);
    check(fah_engine_destroy(session));
    FahResult data;
    check(fah_engine_result_data(result,&data,sizeof data));
    if(data.abi_version!=FAH_ENGINE_ABI || data.hand_count>2) return 7;
    size_t length=0;
    if(fah_engine_result_json(result,NULL,0,&length)!=FAH_BUFFER_TOO_SMALL) return 8;
    char *json=(char*)malloc(length); if(!json) return 9;
    check(fah_engine_result_json(result,json,length,&length));
    puts(json);free(json);
    check(fah_engine_result_destroy(result));
    check(fah_engine_shutdown());
    return 0;
}
