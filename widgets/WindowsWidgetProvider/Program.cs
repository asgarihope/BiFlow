using System.Runtime.InteropServices;
using System.Runtime.Versioning;

namespace BiFlow.WidgetProvider;

/// <summary>
/// Out-of-process COM server entry point.
/// <para>
/// The widget host launches this executable and immediately resolves the
/// provider CLSID declared in <c>Package.appxmanifest</c>. There is no
/// console window, no window, and no UI: the process lives exactly as
/// long as the host needs the class factory.
/// </para>
/// </summary>
[SupportedOSPlatform("windows10.0.17763.0")]
internal static class Program
{
    private const uint CLSCTX_LOCAL_SERVER = 0x4;
    private const uint CLSCTX_INPROC_SERVER = 0x1;
    private const uint REGCLS_SUSPENDED = 0x1;
    private const uint COINIT_APARTMENTTHREADED = 0x2;
    private const int RPC_E_CHANGED_MODE = unchecked((int)0x80010106);

    [STAThread]
    private static int Main()
    {
        // A widget provider is a *Windows* subsystem binary: a console
        // app would flash a black window over the user's desktop every
        // time the board refreshes a card.
        HideConsoleWindow();

        // The host activates us in a single-threaded apartment, so match
        // it. RPC_E_CHANGED_MODE means the host picked a different
        // apartment than we asked for; there is nothing to recover, so
        // fail the activation rather than deadlock.
        int hr = CoInitializeEx(IntPtr.Zero, COINIT_APARTMENTTHREADED);
        if (hr < 0)
        {
            return hr;
        }

        try
        {
            object factory = new WidgetProviderFactory();

            int registered = CoRegisterClassObject(
                ProviderClsid,
                factory,
                CLSCTX_LOCAL_SERVER | CLSCTX_INPROC_SERVER,
                REGCLS_SUSPENDED,
                out uint cookie);

            if (registered < 0 || cookie == 0)
            {
                return registered < 0 ? registered : -1;
            }

            // Resume registration only after the factory is ready, or the
            // host can call in during the gap and get CLASS_E_CLASSNOTAVAIL.
            int resumed = CoResumeClassObjects();
            if (resumed < 0)
            {
                CoRevokeClassObject(cookie);
                return resumed;
            }

            try
            {
                // The host's calls arrive as COM messages. A bare pump is
                // what keeps the factory alive long enough to serve them;
                // the host ends the session by terminating the process.
                while (true)
                {
                    int pump = GetMessage(out NativeMessage message, IntPtr.Zero, 0, 0);
                    if (pump is <= 0 or -1)
                    {
                        break;
                    }

                    TranslateMessage(ref message);
                    DispatchMessage(ref message);
                }
            }
            finally
            {
                CoRevokeClassObject(cookie);
            }

            return 0;
        }
        finally
        {
            CoUninitialize();
        }
    }

    private static Guid ProviderClsid => new(WidgetIds.ProviderClsid);

    [StructLayout(LayoutKind.Sequential)]
    private struct NativeMessage
    {
        public IntPtr hwnd;
        public uint message;
        public IntPtr wParam;
        public IntPtr lParam;
        public uint time;
        public int ptX;
        public int ptY;
    }

    [DllImport("kernel32.dll")]
    private static extern IntPtr GetConsoleWindow();

    [DllImport("kernel32.dll")]
    private static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);

    [DllImport("user32.dll")]
    private static extern int GetMessage(
        out NativeMessage lpMsg,
        IntPtr hWnd,
        uint wMsgFilterMin,
        uint wMsgFilterMax);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool TranslateMessage(ref NativeMessage lpMsg);

    [DllImport("user32.dll")]
    private static extern IntPtr DispatchMessage(ref NativeMessage lpMsg);

    [DllImport("ole32.dll")]
    private static extern int CoInitializeEx(IntPtr pvReserved, uint dwCoInit);

    [DllImport("ole32.dll")]
    private static extern void CoUninitialize();

    [DllImport("ole32.dll")]
    [return: MarshalAs(UnmanagedType.Error)]
    private static extern int CoRegisterClassObject(
        [MarshalAs(UnmanagedType.LPStruct)] Guid rclsid,
        [MarshalAs(UnmanagedType.IUnknown)] object pUnk,
        uint dwClsContext,
        uint flags,
        out uint lpdwRegister);

    [DllImport("ole32.dll")]
    [return: MarshalAs(UnmanagedType.Error)]
    private static extern int CoRevokeClassObject(uint dwRegister);

    [DllImport("ole32.dll")]
    [return: MarshalAs(UnmanagedType.Error)]
    private static extern int CoResumeClassObjects();

    private static void HideConsoleWindow()
    {
        // SW_HIDE = 0. Best effort: a provider launched by the host has no
        // console to begin with, but a manual launch from Explorer does.
        try
        {
            var console = GetConsoleWindow();
            if (console != IntPtr.Zero)
            {
                ShowWindow(console, 0);
            }
        }
        catch (DllNotFoundException)
        {
            // Non-Windows host: nothing to hide.
        }
        catch (EntryPointNotFoundException)
        {
        }
    }
}
