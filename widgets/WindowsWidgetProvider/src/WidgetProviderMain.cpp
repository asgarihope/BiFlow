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

#include <windows.h>
#include <objbase.h>
#include <unknwn.h>

#include <filesystem>
#include <fstream>
#include <string>

#include <winrt/base.h>
#include <winrt/Windows.Foundation.h>
#include <winrt/Microsoft.Windows.Widgets.h>
#include <winrt/Microsoft.Windows.Widgets.Providers.h>

namespace
{
    // Diagnostics. This process has no user-facing surface, so a swallowed
    // failure is indistinguishable from a broken widget: the board just keeps
    // showing its skeleton forever. Every lifecycle callback and every HRESULT
    // goes to %TEMP%\biflow-widget.log so a silent card can be explained.
    void trace(std::string const& message)
    {
        // std::getenv returns char*, not wchar_t*, so go through
        // std::filesystem rather than widening the value by hand.
        std::wstring path = L"biflow-widget.log";
        try
        {
            path = std::filesystem::temp_directory_path() / L"biflow-widget.log";
        }
        catch (...)
        {
            // No temp directory: fall back to the working directory and keep going.
        }
        SYSTEMTIME now{};
        GetLocalTime(&now);
        char stamp[32];
        sprintf_s(stamp, "%02d:%02d:%02d.%03d", now.wHour, now.wMinute, now.wSecond, now.wMilliseconds);
        std::ofstream out(path, std::ios::app);
        if (out)
        {
            out << stamp << " [pid " << GetCurrentProcessId() << "] " << message << "\n";
        }
    }

    std::string hex(int32_t code)
    {
        char buffer[16];
        sprintf_s(buffer, "0x%08X", static_cast<unsigned>(code));
        return buffer;
    }

    void writeStdout(std::string const& text)
    {
        DWORD written = 0;
        ::WriteFile(::GetStdHandle(STD_OUTPUT_HANDLE), text.data(),
                    static_cast<DWORD>(text.size()), &written, nullptr);
    }

    // `--dump-card` prints exactly the payload UpdateWidget would carry, for
    // both card sizes. It exists because the shell is the only thing that ever
    // launches this process, and the shell launches it only once a widget is
    // pinned - so on a machine with nothing pinned there is otherwise no way
    // to see, let alone validate, the card. WriteFile covers both a real
    // console and a redirected pipe, which stdout alone would not.
    int dumpCard()
    {
        ::AttachConsole(ATTACH_PARENT_PROCESS);
        ::SetConsoleOutputCP(CP_UTF8);

        auto snapshot = BiFlow::Widget::readSnapshot();
        writeStdout("connected   : " + std::string(snapshot.connected ? "yes" : "no") + "\n");
        writeStdout("downloaded  : " + std::to_string(snapshot.downloadedTotal) + "\n");
        writeStdout("uploaded    : " + std::to_string(snapshot.uploadedTotal) + "\n");
        writeStdout("route label : " + winrt::to_string(snapshot.defaultRouteLabel) + "\n");
        writeStdout("reason      : " + winrt::to_string(snapshot.failureReason) + "\n");

        for (bool withTotals : { true, false })
        {
            writeStdout(std::string("\n--- card: ") + (withTotals ? "medium" : "small") + " ---\n");
            writeStdout(winrt::to_string(BiFlow::Widget::buildCard(snapshot, withTotals)));
            writeStdout("\n");
        }
        return 0;
    }
}

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
            trace("CreateWidget");
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
            trace("Activate");
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
                trace("push: no context");
                return;
            }
            auto widgetId = widgetContext.Id();
            if (widgetId.empty())
            {
                trace("push: empty widget id");
                return;
            }

            try
            {
                auto snapshot = readSnapshot();
                trace("snapshot connected=" + std::string(snapshot.connected ? "1" : "0") +
                      " down=" + std::to_string(snapshot.downloadedTotal) +
                      " up=" + std::to_string(snapshot.uploadedTotal) +
                      " reason=" + winrt::to_string(snapshot.failureReason));

                // Adaptive Cards scale themselves, but the medium card carries the
                // two-column totals and the routing caption that would not survive
                // the small surface, so ask the host which one it has.
                bool withTotals = widgetContext.Size() != ws::WidgetSize::Small;
                auto card = buildCard(snapshot, withTotals);
                auto narrow = winrt::to_string(card);
                trace("card(" + std::to_string(narrow.size()) + "): " + narrow.substr(0, 240));

                auto manager = wp::WidgetManager::GetDefault();
                trace("WidgetManager::GetDefault ok");

                wp::WidgetUpdateRequestOptions options{widgetId};
                options.Data(card);
                options.Template(card);
                options.IsPlaceholderContent(false);
                manager.UpdateWidget(options);
                trace("UpdateWidget ok");
            }
            catch (winrt::hresult_error const& error)
            {
                trace("hresult_error " + hex(error.code().value) + ": " + winrt::to_string(error.message()));
            }
            catch (std::exception const& error)
            {
                trace(std::string("std::exception: ") + error.what());
            }
            catch (...)
            {
                trace("unknown exception");
            }
        }
    };

    struct ProviderFactory : winrt::implements<ProviderFactory, ::IClassFactory, ILifetimeClassFactory>
    {
        HRESULT __stdcall CreateInstance(::IUnknown* outer, REFIID iid, void** result) noexcept
        {
            trace("IClassFactory::CreateInstance");
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
    if (std::wstring(::GetCommandLineW()).find(L"--dump-card") != std::wstring::npos)
    {
        return dumpCard();
    }

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
        trace("CoRegisterClassObject FAILED");
        return 1;
    }
    trace("CoRegisterClassObject ok, cookie=" + std::to_string(cookie));

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
