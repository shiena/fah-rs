#include "vulkan_ops.h"
#include <layer.h>
#include <pipeline.h>
#include <cstring>

namespace fah {
namespace {
const char* matmulShader=R"glsl(#version 450
layout(binding=0) readonly buffer A { sfp a[]; };
layout(binding=1) readonly buffer B { sfp b[]; };
layout(binding=2) writeonly buffer O { sfp o[]; };
layout(push_constant) uniform parameter { int m;int n;int k;int astep;int bstep;int ostep;int transposeB;int ac;int bc; } p;
shared float atile[16][16];
shared float btile[16][16];
void main() {
    int lx=int(gl_LocalInvocationID.x),ly=int(gl_LocalInvocationID.y);
    int col=int(gl_GlobalInvocationID.x),row=int(gl_GlobalInvocationID.y),batch=int(gl_GlobalInvocationID.z);
    int ao=(p.ac==1?0:batch)*p.astep,bo=(p.bc==1?0:batch)*p.bstep;
    float sum=0;
    for(int base=0;base<p.k;base+=16) {
        int ka=base+lx,kb=base+ly;
        atile[ly][lx]=(row<p.m&&ka<p.k)?float(a[ao+row*p.k+ka]):0;
        btile[ly][lx]=(col<p.n&&kb<p.k)?float(b[bo+(p.transposeB!=0?col*p.k+kb:kb*p.n+col)]):0;
        barrier();
        for(int j=0;j<16;++j) sum+=atile[ly][j]*btile[j][lx];
        barrier();
    }
    if(row<p.m&&col<p.n) o[batch*p.ostep+row*p.n+col]=sfp(sum);
}
)glsl";
const char* reductionShader=R"glsl(#version 450
layout(binding=0) readonly buffer A { sfp a[]; };
layout(binding=1) writeonly buffer O { sfp o[]; };
layout(push_constant) uniform parameter { int width;int height;int axis;int operation;float coefficient; } p;
shared float values[256];
void main() {
    int lane=int(gl_LocalInvocationID.x),outIndex=int(gl_WorkGroupID.y);
    int count=p.axis==0?p.height:p.width;
    float value=p.operation==4?-3.402823466e38:0;
    for(int i=lane;i<count;i+=256) {
        float x=float(a[p.axis==0?i*p.width+outIndex:outIndex*p.width+i]);
        value=p.operation==4?max(value,x):value+x;
    }
    values[lane]=value;barrier();
    for(int stride=128;stride>0;stride/=2) {
        if(lane<stride) values[lane]=p.operation==4?max(values[lane],values[lane+stride]):values[lane]+values[lane+stride];
        barrier();
    }
    if(lane==0) o[outIndex]=sfp(values[0]*p.coefficient/(p.operation==3?float(count):1.0));
}
)glsl";
class ShaderLayer:public ncnn::Layer {
public:
    ncnn::Pipeline* pipeline=nullptr;
    ShaderLayer() { support_vulkan=true;support_vulkan_packing=false; }
    int compile(const char* shader,const ncnn::Option& opt,int x,int y) {
        std::vector<uint32_t> spirv;
        int result=ncnn::compile_spirv_module(shader,int(std::strlen(shader)),opt,spirv);
        if(result) return result;
        pipeline=new ncnn::Pipeline(vkdev);pipeline->set_local_size_xyz(x,y,1);
        return pipeline->create(spirv.data(),spirv.size()*4,{});
    }
    int destroy_pipeline(const ncnn::Option&) override { delete pipeline;pipeline=nullptr;return 0; }
};
// The erf polynomial follows the MLX implementation used by fah-metal (see NOTICE).
const char* geluShader=R"glsl(#version 450
layout(binding=0) buffer A { sfp a[]; };
layout(push_constant) uniform parameter { int n; } p;
float erfApprox(float a) {
    float t=abs(a),s=a*a,r,u;
    if(t>.927734375) {
        r=fma(-1.72853470e-5,t,3.83197126e-4);
        u=fma(-3.88396438e-3,t,2.42546219e-2);
        r=fma(r,s,u);r=fma(r,t,-1.06777877e-1);r=fma(r,t,-6.34846687e-1);
        r=fma(r,t,-1.28717512e-1);r=fma(r,t,-t);
        return sign(a)*(1-exp(r));
    }
    r=-5.96761703e-4;r=fma(r,s,4.99119423e-3);r=fma(r,s,-2.67681349e-2);
    r=fma(r,s,1.12819925e-1);r=fma(r,s,-3.76125336e-1);r=fma(r,s,1.28379166e-1);
    return fma(r,a,a);
}
void main() {
    int i=int(gl_GlobalInvocationID.x);if(i>=p.n) return;
    float x=float(a[i]);a[i]=sfp(.5*x*(1+erfApprox(x*.7071067811865475)));
}
)glsl";
class GELU:public ShaderLayer {
public:
    GELU() { one_blob_only=true;support_inplace=true; }
    int create_pipeline(const ncnn::Option& opt) override { return compile(geluShader,opt,128,1); }
    int forward_inplace(ncnn::VkMat& a,ncnn::VkCompute& cmd,const ncnn::Option&) const override {
        if(a.elempack!=1) return -1;
        int n=a.dims>=3?a.cstep*a.c:a.w*a.h;
        std::vector<ncnn::vk_constant_type> p(1);p[0].i=n;
        ncnn::VkMat dispatch;dispatch.w=n;dispatch.h=1;dispatch.c=1;
        cmd.record_pipeline(pipeline,{a},p,dispatch);return 0;
    }
};
class MatMul:public ShaderLayer {
    int transB=0;
public:
    MatMul() { one_blob_only=false; }
    int load_param(const ncnn::ParamDict& pd) override { transB=pd.get(0,0);return 0; }
    int create_pipeline(const ncnn::Option& opt) override { return compile(matmulShader,opt,16,16); }
    int forward(const std::vector<ncnn::VkMat>& inputs,std::vector<ncnn::VkMat>& outputs,ncnn::VkCompute& cmd,const ncnn::Option& opt) const override {
        const auto& a=inputs[0];const auto& b=inputs[1];auto& out=outputs[0];
        if(a.dims<2||a.dims>3||b.dims<2||b.dims>3||a.elempack!=1||b.elempack!=1) return -1;
        int m=a.h,k=a.w,n=transB?b.h:b.w,c=std::max(a.c,b.c);
        if(k!=(transB?b.w:b.h)||(a.c!=b.c&&a.c!=1&&b.c!=1)) return -1;
        if(a.dims==2&&b.dims==2) out.create(n,m,a.elemsize,1,opt.blob_vkallocator);
        else out.create(n,m,c,a.elemsize,1,opt.blob_vkallocator);
        if(out.empty()) return -100;
        std::vector<ncnn::vk_constant_type> p(9);
        p[0].i=m;p[1].i=n;p[2].i=k;p[3].i=a.cstep;p[4].i=b.cstep;p[5].i=out.cstep;p[6].i=transB;p[7].i=a.c;p[8].i=b.c;
        ncnn::VkMat dispatch;dispatch.w=n;dispatch.h=m;dispatch.c=c;
        cmd.record_pipeline(pipeline,{a,b,out},p,dispatch);
        return 0;
    }
};
class Reduction:public ShaderLayer {
    int operation=0,axis=0;float coefficient=1;
public:
    Reduction() { one_blob_only=true; }
    int load_param(const ncnn::ParamDict& pd) override {
        operation=pd.get(0,0);coefficient=pd.get(2,1.f);
        auto axes=pd.get(3,ncnn::Mat());
        if(pd.get(1,1)!=0||pd.get(4,0)!=0||axes.w!=1||(operation!=0&&operation!=3&&operation!=4)) return -1;
        axis=reinterpret_cast<const int*>(axes.data)[0];return 0;
    }
    int create_pipeline(const ncnn::Option& opt) override { return compile(reductionShader,opt,256,1); }
    int forward(const ncnn::VkMat& a,ncnn::VkMat& out,ncnn::VkCompute& cmd,const ncnn::Option& opt) const override {
        if(a.dims!=2||a.elempack!=1||(axis!=0&&axis!=1)) return -1;
        int n=axis==0?a.w:a.h;
        out.create(n,a.elemsize,1,opt.blob_vkallocator);
        if(out.empty()) return -100;
        std::vector<ncnn::vk_constant_type> p(5);
        p[0].i=a.w;p[1].i=a.h;p[2].i=axis;p[3].i=operation;p[4].f=coefficient;
        ncnn::VkMat dispatch;dispatch.w=256;dispatch.h=n;dispatch.c=1;
        cmd.record_pipeline(pipeline,{a,out},p,dispatch);return 0;
    }
};
DEFINE_LAYER_CREATOR(MatMul)
DEFINE_LAYER_CREATOR(Reduction)
DEFINE_LAYER_CREATOR(GELU)
const char* normShader=R"glsl(#version 450
layout(binding=0) buffer A { sfp a[]; };
layout(binding=1) readonly buffer G { sfp gamma[]; };
layout(binding=2) readonly buffer B { sfp beta[]; };
layout(push_constant) uniform parameter { int width;int height;int cstep;float eps; } p;
shared float values[256];
shared float meanValue;
shared float scaleValue;
void main() {
    int lane=int(gl_LocalInvocationID.x),row=int(gl_WorkGroupID.y),channel=int(gl_WorkGroupID.z);
    int base=channel*p.cstep+row*p.width;
    float sum=0;
    for(int j=lane;j<p.width;j+=256) sum+=float(a[base+j]);
    values[lane]=sum;barrier();
    for(int s=128;s>0;s/=2) { if(lane<s) values[lane]+=values[lane+s];barrier(); }
    if(lane==0) meanValue=values[0]/float(p.width);
    barrier();sum=0;
    for(int j=lane;j<p.width;j+=256) {float d=float(a[base+j])-meanValue;sum+=d*d;}
    values[lane]=sum;barrier();
    for(int s=128;s>0;s/=2) { if(lane<s) values[lane]+=values[lane+s];barrier(); }
    if(lane==0) scaleValue=inversesqrt(values[0]/float(p.width)+p.eps);
    barrier();
    for(int j=lane;j<p.width;j+=256) a[base+j]=sfp((float(a[base+j])-meanValue)*scaleValue*float(gamma[j])+float(beta[j]));
}
)glsl";
class LayerNorm:public ShaderLayer {
    int width=0;float eps=0;
    ncnn::Mat gamma,beta;
    ncnn::VkMat gpuGamma,gpuBeta;
public:
    LayerNorm() { one_blob_only=true;support_inplace=true; }
    int load_param(const ncnn::ParamDict& pd) override {
        width=pd.get(0,0);eps=pd.get(1,.00001f);return pd.get(2,1)==1?0:-1;
    }
    int load_model(const ncnn::ModelBin& mb) override {
        gamma=mb.load(width,1);beta=mb.load(width,1);return gamma.empty()||beta.empty()?-100:0;
    }
    int create_pipeline(const ncnn::Option& opt) override { return compile(normShader,opt,256,1); }
    int upload_model(ncnn::VkTransfer& cmd,const ncnn::Option& opt) override {
        cmd.record_upload(gamma,gpuGamma,opt);cmd.record_upload(beta,gpuBeta,opt);return 0;
    }
    int forward_inplace(ncnn::VkMat& a,ncnn::VkCompute& cmd,const ncnn::Option&) const override {
        if(a.w!=width||a.elempack!=1||a.dims>3) return -1;
        std::vector<ncnn::vk_constant_type> p(4);p[0].i=a.w;p[1].i=a.h;p[2].i=a.cstep;p[3].f=eps;
        ncnn::VkMat dispatch;dispatch.w=256;dispatch.h=a.h;dispatch.c=a.c;
        cmd.record_pipeline(pipeline,{a,gpuGamma,gpuBeta},p,dispatch);return 0;
    }
};
DEFINE_LAYER_CREATOR(LayerNorm)
}
void registerVulkanOps(ncnn::Net& net) {
    net.register_custom_layer("MatMul",MatMul_layer_creator);
    net.register_custom_layer("Reduction",Reduction_layer_creator);
    net.register_custom_layer("GELU",GELU_layer_creator);
    net.register_custom_layer("LayerNorm",LayerNorm_layer_creator);
}
}
