package dev.fah.android;

import android.Manifest;
import android.app.Activity;
import android.content.pm.PackageManager;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.Paint;
import android.graphics.SurfaceTexture;
import android.hardware.Camera;
import android.os.Bundle;
import android.util.Log;
import android.view.Gravity;
import android.view.TextureView;
import android.view.View;
import android.view.WindowManager;
import android.widget.Button;
import android.widget.FrameLayout;
import android.widget.ImageView;
import android.widget.LinearLayout;
import android.widget.TextView;
import org.json.JSONArray;
import org.json.JSONObject;
import java.io.File;
import java.io.FileOutputStream;
import java.nio.charset.StandardCharsets;
import java.util.Locale;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

@SuppressWarnings("deprecation")
public class MainActivity extends Activity implements TextureView.SurfaceTextureListener {
    static { System.loadLibrary("fah"); }
    private native String nativeInit(String path,boolean gpu);
    private native String nativeProcess(Bitmap image, boolean reset);
    private native void nativeClose();
    private final ExecutorService worker = Executors.newSingleThreadExecutor();
    private TextureView preview;
    private ImageView sampleView;
    private Overlay overlay;
    private TextView status;
    private Camera camera;
    private volatile boolean ready, busy, destroyed;
    private boolean resumed, sampleMode, reset = true;
    private File modelDir;
    private int generation;
    private boolean useGpu=true;
    private boolean cpuFallback;
    private TextView title;

    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
        modelDir = new File(getExternalFilesDir(null), "models");
        sampleMode = getIntent().getBooleanExtra("sample", false);
        useGpu = !getIntent().getBooleanExtra("cpu", false);
        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setBackgroundColor(Color.rgb(15, 20, 27));
        title = new TextView(this);
        title.setText(useGpu?"FAH Android  ·  Vulkan":"FAH Android  ·  CPU");
        title.setTextSize(22); title.setTextColor(Color.WHITE); title.setPadding(24, 24, 24, 16);
        root.addView(title);
        status = new TextView(this);
        status.setText("モデルを読み込み中…"); status.setTextColor(Color.LTGRAY);
        status.setTextSize(14); status.setPadding(24, 0, 24, 16);
        root.addView(status);
        FrameLayout stage = new FrameLayout(this);
        preview = new TextureView(this); preview.setSurfaceTextureListener(this);
        sampleView = new ImageView(this); sampleView.setScaleType(ImageView.ScaleType.FIT_CENTER);
        overlay = new Overlay();
        stage.addView(preview, new FrameLayout.LayoutParams(-1, -1));
        stage.addView(sampleView, new FrameLayout.LayoutParams(-1, -1));
        stage.addView(overlay, new FrameLayout.LayoutParams(-1, -1));
        stage.addOnLayoutChangeListener((v,l,t,r,b,ol,ot,or,ob) -> {
            int height=Math.min(b-t,(r-l)*4/3),width=height*3/4;
            FrameLayout.LayoutParams lp=(FrameLayout.LayoutParams)preview.getLayoutParams();
            if(lp.width!=width||lp.height!=height) {
                lp.width=width;lp.height=height;lp.gravity=Gravity.CENTER;preview.setLayoutParams(lp);
            }
        });
        root.addView(stage, new LinearLayout.LayoutParams(-1, 0, 1));
        LinearLayout buttons = new LinearLayout(this);
        Button live = new Button(this); live.setText("カメラ");
        Button sample = new Button(this); sample.setText("サンプル検証");
        Button detect = new Button(this); detect.setText("再検出");
        Button backend = new Button(this); backend.setText("CPU/GPU");
        buttons.addView(live, new LinearLayout.LayoutParams(0, -2, 1));
        buttons.addView(sample, new LinearLayout.LayoutParams(0, -2, 1));
        buttons.addView(detect, new LinearLayout.LayoutParams(0, -2, 1));
        buttons.addView(backend, new LinearLayout.LayoutParams(0, -2, 1));
        root.addView(buttons);
        TextView note = new TextView(this);
        note.setText("21関節・最大2手 / メッシュ・接触推定なし\n3D座標には推定カメラ内部パラメーターを使用");
        note.setTextColor(Color.GRAY); note.setPadding(24, 8, 24, 16); root.addView(note);
        setContentView(root);
        live.setOnClickListener(v -> { generation++; sampleMode=false; reset=true; overlay.result=null; sampleView.setVisibility(View.GONE); startCamera(); });
        sample.setOnClickListener(v -> { generation++; sampleMode=true; stopCamera(); showSample(); });
        detect.setOnClickListener(v -> { reset=true; if(sampleMode) showSample(); });
        backend.setOnClickListener(v -> {
            if(!ready) return;
            generation++;ready=false;stopCamera();useGpu=!useGpu;
            title.setText(useGpu?"FAH Android  ·  Vulkan":"FAH Android  ·  CPU");
            loadModels();
        });
        loadModels();
    }
    private void loadModels() {
        cpuFallback=false;
        status.setText("モデルを読み込み中…");
        worker.execute(() -> {
            try {
                String info=nativeInit(modelDir.getAbsolutePath(),useGpu);
                Log.i("FAH", "GPU " + info);
                ready=true;
                runOnUiThread(() -> { if(destroyed) return; if(sampleMode) showSample(); else startCamera(); });
            } catch(Exception e) { reportError(e); }
        });
    }

    private void reportError(Exception e) {
        Log.e("FAH", "Inference error", e);
        runOnUiThread(() -> { if(!destroyed) status.setText("エラー: " + e.getMessage()); });
    }
    private void startCamera() {
        if(!resumed||!ready||sampleMode||!preview.isAvailable()||camera!=null) return;
        if(checkSelfPermission(Manifest.permission.CAMERA)!=PackageManager.PERMISSION_GRANTED) {
            requestPermissions(new String[]{Manifest.permission.CAMERA}, 1); return;
        }
        try {
            sampleView.setVisibility(View.GONE);
            camera=Camera.open();
            Camera.Parameters p=camera.getParameters();
            Camera.Size selected=p.getSupportedPreviewSizes().get(0);
            for(Camera.Size s:p.getSupportedPreviewSizes()) if(s.width==640&&s.height==480) selected=s;
            p.setPreviewSize(selected.width,selected.height);
            if(p.getSupportedFocusModes().contains(Camera.Parameters.FOCUS_MODE_CONTINUOUS_VIDEO))
                p.setFocusMode(Camera.Parameters.FOCUS_MODE_CONTINUOUS_VIDEO);
            camera.setParameters(p); camera.setDisplayOrientation(90);
            camera.setPreviewTexture(preview.getSurfaceTexture());camera.startPreview();
            status.setText("手をカメラに向けてください"); reset=true;
        } catch(Exception e) { stopCamera();reportError(e); }
    }
    private void stopCamera() {
        if(camera!=null) { camera.stopPreview();camera.release();camera=null; }
    }
    private void showSample() {
        if(!ready||busy||destroyed) return;
        BitmapFactory.Options options=new BitmapFactory.Options();options.inPreferredConfig=Bitmap.Config.ARGB_8888;
        Bitmap bitmap=BitmapFactory.decodeFile(new File(modelDir,"sample.png").getAbsolutePath(),options);
        if(bitmap==null) { status.setText("models/sample.png がありません");return; }
        sampleView.setVisibility(View.VISIBLE);sampleView.setImageBitmap(bitmap);
        status.setText("サンプルを推論中…");
        submit(bitmap,true,true);
    }
    private void submit(Bitmap bitmap,boolean sample,boolean restart) {
        if(busy||destroyed) { if(!sample) bitmap.recycle();return; }
        busy=true;
        // This device's 2021 Adreno driver can deadlock when camera rendering overlaps long compute work.
        // Keep the sampled frame visible and resume preview after its inference completes.
        boolean pausedPreview=!sample&&useGpu&&!cpuFallback&&camera!=null;
        if(pausedPreview) camera.stopPreview();
        int current=generation;
        worker.execute(() -> {
            try {
                String json=nativeProcess(bitmap,restart);
                JSONObject result=new JSONObject(json);
                try(FileOutputStream f=new FileOutputStream(new File(getExternalFilesDir(null),"last-result.json"))) {
                    f.write(json.getBytes(StandardCharsets.UTF_8));
                }
                Log.i("FAH", "RESULT " + json);
                runOnUiThread(() -> {
                    if(destroyed||generation!=current) return;
                    overlay.result=result;overlay.fit=true;overlay.invalidate();
                    cpuFallback=result.optBoolean("cpu_fallback");
                    if(cpuFallback) title.setText("FAH Android  ·  CPU（GPUエラー後）");
                    double ms=result.optDouble("total_ms");
                    status.setText(String.format(Locale.US,"%d手  ·  %.1f ms / %.1f 推論FPS\n検出 %.1f ms  ·  姿勢 %.1f ms  ·  %s",
                        result.optJSONArray("hands").length(),ms,1000/ms,result.optDouble("detector_ms"),result.optDouble("pose_ms"),
                        result.optBoolean("detected")?"再検出":"追跡"));
                    if(sample) saveSample(bitmap,result);
                });
            } catch(Exception e) { ready=false;reportError(e); }
            finally {
                if(!sample) bitmap.recycle();
                runOnUiThread(() -> {
                    busy=false;
                    if(pausedPreview&&ready&&resumed&&!destroyed&&!sampleMode&&camera!=null&&generation==current) {
                        try { camera.startPreview(); } catch(Exception e) { reportError(e); }
                    }
                });
            }
        });
    }
    private void saveSample(Bitmap image,JSONObject result) {
        try {
            Bitmap rendered=image.copy(Bitmap.Config.ARGB_8888,true);
            Canvas c=new Canvas(rendered);overlay.drawHands(c,result,1,1,0,0);
            try(FileOutputStream f=new FileOutputStream(new File(getExternalFilesDir(null),"sample-result.png"))) {
                rendered.compress(Bitmap.CompressFormat.PNG,100,f);
            }
            rendered.recycle();
        } catch(Exception e) { reportError(e); }
    }
    @Override protected void onResume() { super.onResume();resumed=true;startCamera(); }
    @Override protected void onPause() { resumed=false;generation++;stopCamera();super.onPause(); }
    @Override protected void onDestroy() { destroyed=true;worker.execute(this::nativeClose);worker.shutdown();super.onDestroy(); }
    @Override public void onRequestPermissionsResult(int request,String[] permissions,int[] grants) {
        super.onRequestPermissionsResult(request,permissions,grants);
        if(grants.length>0&&grants[0]==PackageManager.PERMISSION_GRANTED) startCamera();
        else status.setText("カメラの使用には権限が必要です。サンプル検証は利用できます。");
    }
    @Override public void onSurfaceTextureAvailable(SurfaceTexture s,int w,int h) { startCamera(); }
    @Override public void onSurfaceTextureSizeChanged(SurfaceTexture s,int w,int h) { reset=true; }
    @Override public boolean onSurfaceTextureDestroyed(SurfaceTexture s) { stopCamera();return true; }
    @Override public void onSurfaceTextureUpdated(SurfaceTexture s) {
        if(!ready||busy||sampleMode||!resumed||destroyed) return;
        Bitmap frame=preview.getBitmap(480,640);
        if(frame!=null) { boolean restart=reset;reset=false;submit(frame,false,restart); }
    }
    private class Overlay extends View {
        JSONObject result;
        boolean fit;
        final Paint paint=new Paint(Paint.ANTI_ALIAS_FLAG);
        final int[][] fingers={{0,1,2,3,4},{0,5,6,7,8},{0,9,10,11,12},{0,13,14,15,16},{0,17,18,19,20}};
        final int[] colors={0xffffc857,0xffff6b6b,0xff6ae3b6,0xff62b6ff,0xffce8eff};
        Overlay() { super(MainActivity.this);setWillNotDraw(false); }
        void drawHands(Canvas c,JSONObject data,float sx,float sy,float ox,float oy) throws Exception {
            JSONArray hands=data.getJSONArray("hands");
            for(int h=0;h<hands.length();++h) {
                JSONArray joints=hands.getJSONObject(h).getJSONArray("joints2d");
                for(int finger=0;finger<5;++finger) {
                    paint.setColor(colors[finger]);paint.setStrokeWidth(3);
                    for(int j=1;j<5;++j) {
                        JSONArray a=joints.getJSONArray(fingers[finger][j-1]),b=joints.getJSONArray(fingers[finger][j]);
                        c.drawLine(ox+(float)a.getDouble(0)*sx,oy+(float)a.getDouble(1)*sy,
                                   ox+(float)b.getDouble(0)*sx,oy+(float)b.getDouble(1)*sy,paint);
                    }
                }
                paint.setColor(Color.WHITE);
                for(int j=0;j<21;++j) { JSONArray p=joints.getJSONArray(j);c.drawCircle(ox+(float)p.getDouble(0)*sx,oy+(float)p.getDouble(1)*sy,3,paint); }
            }
        }
        @Override protected void onDraw(Canvas c) {
            super.onDraw(c);if(result==null) return;
            try {
                float w=result.getInt("width"),h=result.getInt("height"),sx=getWidth()/w,sy=getHeight()/h,ox=0,oy=0;
                if(fit) { sx=sy=Math.min(sx,sy);ox=(getWidth()-w*sx)/2;oy=(getHeight()-h*sy)/2; }
                drawHands(c,result,sx,sy,ox,oy);
            } catch(Exception e) { Log.e("FAH","Overlay",e); }
        }
    }
}
