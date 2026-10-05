using System.Runtime.InteropServices;

namespace BiFlow.WidgetProvider;

/// <summary>
/// Minimal <c>IClassFactory</c> for the widget provider.
/// <para>
/// The Windows Widgets Board activates the provider out of process:
/// it launches this executable and calls <c>CoCreateInstance</c> for the
/// provider CLSID. A class factory is what makes that call resolve to a
/// managed <see cref="WidgetProvider"/> instance.
/// </para>
/// <para>
/// The implementation is the one the official Windows App SDK widget
/// sample uses: a managed class that projects itself as a COM class
/// factory through <c>ComWrappersSupport</c>, so no hand-written vtable
/// or <c>unsafe</c> pointer juggling is required.
/// </para>
/// </summary>
[ComVisible(true)]
internal sealed class WidgetProviderFactory : ComWrappersSupport
{
    public static IntPtr CreateComInterfaceForObject(object instance) =>
        ComWrappersSupport.GetOrCreateComInterfaceForObject(instance, CreateComInterface);

    private static IntPtr CreateComInterfaceForObject(
        ComWrappersSupport wrappersSupport,
        object instance)
    {
        // AgileObject: the host may call from a different apartment than
        // the one the process registered on.
        var vftbl = wrappersSupport.GetVftblForComInterface(typeof(IClassFactory));
        var wrapper = wrappersSupport.CreateObjectNativeWrapper(instance);
        return wrappersSupport.CreateComInterfaceWrapper(vftbl, wrapper);
    }
}

/// <summary>The two methods COM requires of a class factory.</summary>
[ComVisible(true)]
[Guid("00000001-0000-0000-C000-000000000046")]
[InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
internal interface IClassFactory
{
    void CreateInstance(
        [MarshalAs(UnmanagedType.IUnknown)] object outer,
        ref Guid classId,
        [MarshalAs(UnmanagedType.Interface)] out object instance);

    void LockServer(bool lockServer);
}
