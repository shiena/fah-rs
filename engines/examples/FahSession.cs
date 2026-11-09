using System;
using System.Runtime.InteropServices;
using System.Text;

// Call ProcessJson on a host worker; marshal results back to the engine main thread.
public sealed class FahSession : IDisposable
{
#if UNITY_IOS && !UNITY_EDITOR
    private const string Library = "__Internal";
#else
    private const string Library = "fah_engine";
#endif
    [StructLayout(LayoutKind.Sequential)]
    private struct Config { public uint Version, Size; public IntPtr Models, Native; public uint Backend; }
    [StructLayout(LayoutKind.Sequential)]
    private struct Image { public IntPtr Pixels; public UIntPtr Length; public uint Width, Height; public UIntPtr Stride; public uint Reset; }
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] private static extern uint fah_engine_abi_version();
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] private static extern int fah_engine_create(ref Config config, out ulong session);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] private static extern int fah_engine_destroy(ulong session);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] private static extern int fah_engine_process(ulong session, ref Image image, out ulong result);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] private static extern int fah_engine_result_json(ulong result, [Out] byte[] bytes, UIntPtr capacity, out UIntPtr required);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] private static extern int fah_engine_result_destroy(ulong result);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] private static extern UIntPtr fah_engine_last_error([Out] byte[] bytes, UIntPtr capacity);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] private static extern int fah_engine_shutdown();
    // Call once after all sessions are disposed and all worker tasks have joined.
    public static void Shutdown() { Check(fah_engine_shutdown()); }
    private readonly object gate = new object();
    private ulong handle;
    private static IntPtr Utf8(string value)
    {
        if (value == null) return IntPtr.Zero;
        if (value.IndexOf('\0') >= 0) throw new ArgumentException("Embedded NUL");
        byte[] bytes = Encoding.UTF8.GetBytes(value + "\0");
        IntPtr memory = Marshal.AllocHGlobal(bytes.Length);
        Marshal.Copy(bytes, 0, memory, bytes.Length);
        return memory;
    }
    private static void Check(int status)
    {
        if (status == 0) return;
        int length = checked((int)fah_engine_last_error(null, UIntPtr.Zero).ToUInt64());
        byte[] error = new byte[length];
        fah_engine_last_error(error, (UIntPtr)error.Length);
        throw new InvalidOperationException(Encoding.UTF8.GetString(error, 0, Math.Max(0, length - 1)));
    }
    // Backend: auto=0, CPU=1, Vulkan=2, CUDA=3, DirectML=4, Metal=5.
    // macOS requires the absolute path to libFAHAppleNative.dylib.
    public FahSession(string models, uint backend = 0, string nativeLibrary = null)
    {
        if (fah_engine_abi_version() != 1) throw new NotSupportedException("FAH ABI mismatch");
        IntPtr modelPath = Utf8(models), nativePath = IntPtr.Zero;
        try
        {
            nativePath = Utf8(nativeLibrary);
            Config config = new Config { Version = 1, Size = (uint)Marshal.SizeOf<Config>(), Models = modelPath, Native = nativePath, Backend = backend };
            Check(fah_engine_create(ref config, out handle));
        }
        finally { Marshal.FreeHGlobal(modelPath); Marshal.FreeHGlobal(nativePath); }
    }
    // RGBA8 rows start at the top left. Rotate/flip engine textures before calling.
    public string ProcessJson(byte[] rgba, uint width, uint height, uint stride, bool reset = false)
    {
        if (rgba == null) throw new ArgumentNullException(nameof(rgba));
        lock (gate)
        {
            if (handle == 0) throw new ObjectDisposedException(nameof(FahSession));
            GCHandle pixels = GCHandle.Alloc(rgba, GCHandleType.Pinned);
            ulong result = 0;
            try
            {
                Image image = new Image { Pixels = pixels.AddrOfPinnedObject(), Length = (UIntPtr)rgba.Length, Width = width, Height = height, Stride = (UIntPtr)stride, Reset = reset ? 1u : 0u };
                Check(fah_engine_process(handle, ref image, out result));
                UIntPtr required;
                int status = fah_engine_result_json(result, null, UIntPtr.Zero, out required);
                if (status != 2) { Check(status); throw new InvalidOperationException("Invalid JSON sizing response"); }
                byte[] bytes = new byte[checked((int)required.ToUInt64())];
                Check(fah_engine_result_json(result, bytes, (UIntPtr)bytes.Length, out required));
                return Encoding.UTF8.GetString(bytes, 0, bytes.Length - 1);
            }
            finally { if (result != 0) fah_engine_result_destroy(result); pixels.Free(); }
        }
    }
    public void Dispose()
    {
        lock (gate) { if (handle != 0) { ulong old = handle; handle = 0; Check(fah_engine_destroy(old)); } }
    }
}
