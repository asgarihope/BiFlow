// BiFlow Windows 11 Widgets Board provider.
//
// BiFlow itself is a Tauri v2 app (Rust + React) and cannot implement
// IWidgetProvider: that is a WinRT/COM contract the Windows App SDK
// activates. So this is a headless, UI-less out-of-process COM server
// that the Widgets host starts on demand and calls back through COM.
//
// It owns NO state and NO privilege. Every value on the card comes from
// the same two sources the desktop already uses:
//   * %APPDATA%\biflow\config.toml   (controller endpoint + clients)
//   * the local Mihomo REST controller (live counters)
//
// Nothing here launches, links against, or requires the BiFlow desktop.

// <unknwn.h> must precede every C++/WinRT header, because C++/WinRT decides at
// include time whether classic COM interfaces are available. The local headers
// pull in winrt/base.h, so the Win32 and COM headers come first - not as a
// style preference but because the order is load-bearing.
#include <windows.h>
#include <objbase.h>
#include <unknwn.h>

#include "BiFlowData.h"
#include "WidgetIds.h"

#include <winrt/base.h>
#include <winrt/Windows.Foundation.h>
#include <winrt/Microsoft.Windows.Widgets.h>
#include <winrt/Microsoft.Windows.Widgets.Providers.h>

namespace wp = winrt::Microsoft::Windows::Widgets::Providers;
namespace ws = winrt::Microsoft::Windows::Widgets;

namespace
{
    // <unknwn.h> already declares IClassFactory; declaring our own would shadow
    // it and winrt::implements would then build the vtable against the SDK's
    // pure virtual. ILifetimeClassFactory is genuinely absent from the SDK, so
    // it is the one interface declared here - __declspec(uuid) is what lets
    // winrt::implements build the vtable and answer QueryInterface with the
    // real IID, instead of us hand-writing thunks.
    struct __declspec(uuid("e5d8a025-bc73-4e6b-8f55-370f950bc1ce")) ILifetimeClassFactory : public ::IUnknown
    {
        virtual HRESULT __stdcall GetInstanceCount(uint32_t* count) noexcept = 0;
        virtual void __stdcall ReleaseFactory() noexcept = 0;
    };
}

namespace BiFlow::Widget
{
    struct Provider : winrt::implements<Provider, wp::IWidgetProvider>
    {
        // The host asks for content when a widget is first pinned.
        void CreateWidget(wp::WidgetContext const& widgetContext)
        {
            push(widgetContext);
        }

        // Required by the interface. The card has no per-widget state to tear
        // down, and the host drops its own copy of the id for us.
        void DeleteWidget(winrt::param::hstring const& widgetId, winrt::param::hstring const& customState)
        {
        }

        // The card carries no actionable controls, so there is nothing to
        // invoke and no reply to send.
        void OnActionInvoked(wp::WidgetActionInvokedArgs const& actionInvokedArgs)
        {
        }

        // Theme, host surface, or size changed. The card is theme-neutral - the
        // host applies light and dark itself - so a re-render is enough to
        // repaint into the new container, at the new size.
        void OnWidgetContextChanged(wp::WidgetContextChangedArgs const& contextChangedArgs)
        {
            if (contextChangedArgs)
            {
                push(contextChangedArgs.WidgetContext());
            }
        }

        // The widget became visible: refresh immediately rather than making
        // the user wait out the host's cadence.
        void Activate(wp::WidgetContext const& widgetContext)
        {
            push(widgetContext);
        }

        void Deactivate(winrt::param::hstring const& widgetId)
        {
        }

    private:
        void push(wp::WidgetContext const& widgetContext)
        {
            if (!widgetContext)
            {
                return;
            }
            auto widgetId = widgetContext.Id();
            if (widgetId.empty())
            {
                return;
            }

            auto snapshot = readSnapshot();
            // Adaptive Cards scale themselves, but the medium card carries the
            // two-column totals and the routing caption that would not survive
            // the small surface, so ask the host which one it has.
            bool withTotals = widgetContext.Size() != ws::WidgetSize::Small;
            auto card = buildCard(snapshot, withTotals);

            try
            {
                wp::WidgetUpdateRequestOptions options{widgetId};
                options.Data(card);
                options.Template(card);
                options.IsPlaceholderContent(false);
                wp::WidgetManager::GetDefault().UpdateWidget(options);
            }
            catch (winrt::hresult_error const&)
            {
                // Deliberately silent. This process has no user-facing surface
                // and no log consumer. A controller failure is reported
                // through the card itself, which then reads "Disconnected"
                // with the reason underneath.
            }
        }
    };

    struct ProviderFactory : winrt::implements<ProviderFactory, ::IClassFactory, ILifetimeClassFactory>
    {
        HRESULT __stdcall CreateInstance(::IUnknown* outer, REFIID iid, void** result) noexcept
        {
            if (result)
            {
                *result = nullptr;
            }
            if (outer)
            {
                return E_NOINTERFACE;  // aggregation is never requested
            }
            try
            {
                auto instance = winrt::make<Provider>();
                auto unknown = instance.as<::IUnknown>();
                // The host asks for IWidgetProvider, but let it decide rather
                // than hard-coding the IID.
                return unknown->QueryInterface(iid, result);
            }
            catch (winrt::hresult_error const& error)
            {
                return error.code().value;
            }
            catch (...)
            {
                return E_UNEXPECTED;
            }
        }

        HRESULT __stdcall LockServer(BOOL) noexcept
        {
            return S_OK;  // the process lifetime is owned by the shell
        }

        HRESULT __stdcall GetInstanceCount(uint32_t* count) noexcept
        {
            if (count)
            {
                *count = 0;
            }
            return S_OK;
        }

        void __stdcall ReleaseFactory() noexcept
        {
        }
    };
}

int WINAPI wWinMain(HINSTANCE, HINSTANCE, PWSTR, int)
{
    winrt::init_apartment();

    auto factory = winrt::make<BiFlow::Widget::ProviderFactory>();
    auto classFactory = factory.as<::IUnknown>();

    // winrt::guid converts to the Win32 GUID only explicitly, so hand
    // CoRegisterClassObject the type its prototype actually declares.
    ::GUID clsid = static_cast<::GUID>(BiFlow::Widget::ProviderClsid);

    ULONG cookie{};
    // The App-family overload of CoRegisterClassObject carries a REGCLS-style
    // flags argument that the Desktop one does not. Pass 0: the shell owns
    // the registration lifetime and we never re-register in-process.
    if (FAILED(CoRegisterClassObject(clsid, classFactory.get(), CLSCTX_LOCAL_SERVER, 0, &cookie)))
    {
        return 1;
    }

    // Stay alive until the shell releases the last factory reference.
    MSG message{};
    while (::GetMessageW(&message, nullptr, 0, 0) > 0)
    {
        ::TranslateMessage(&message);
        ::DispatchMessageW(&message);
    }

    CoRevokeClassObject(cookie);
    return 0;
}
