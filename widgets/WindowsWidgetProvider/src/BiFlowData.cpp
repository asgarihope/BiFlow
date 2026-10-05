#include "BiFlowData.h"

#include <windows.h>
#include <shlobj.h>

#include <winrt/base.h>
#include <winrt/Windows.Foundation.h>
#include <winrt/Windows.Networking.Sockets.h>
#include <winrt/Windows.Storage.Streams.h>

#include <algorithm>
#include <cmath>
#include <fstream>
#include <sstream>
#include <vector>

using namespace winrt;
using namespace winrt::Windows::Foundation;
using namespace winrt::Windows::Networking::Sockets;
using namespace winrt::Windows::Storage::Streams;

namespace BiFlow::Widget
{
    namespace
    {
        constexpr wchar_t const* AppFolder = L"biflow";
        constexpr wchar_t const* ConfigFile = L"config.toml";

        // Trim ASCII whitespace from both ends.
        std::wstring trim(std::wstring_view text)
        {
            auto isSpace = [](wchar_t c) { return c == L' ' || c == L'\t' || c == L'\r' || c == L'\n'; };
            while (!text.empty() && isSpace(text.front()))
            {
                text.remove_prefix(1);
            }
            while (!text.empty() && isSpace(text.back()))
            {
                text.remove_suffix(1);
            }
            return std::wstring{text};
        }

        bool startsWith(std::wstring_view text, std::wstring_view prefix)
        {
            return text.size() >= prefix.size() && text.compare(0, prefix.size(), prefix) == 0;
        }

        // The one rule for reading `key = value` out of a config.toml line.
        // Whitespace around the '=' is optional, because the Rust writer emits
        // `controller_secret = "..."` with spaces: a parser that only accepts
        // `key=value` silently finds nothing, and the defaults (127.0.0.1,
        // 19090, no secret) look plausible right up until the controller
        // answers 401. Every key in this file must go through here.
        std::wstring tomlValue(std::wstring_view line, std::wstring_view key)
        {
            if (!startsWith(line, key))
            {
                return {};
            }
            auto at = key.size();
            while (at < line.size() && (line[at] == L' ' || line[at] == L'\t'))
            {
                ++at;
            }
            if (at >= line.size() || line[at] != L'=')
            {
                return {};
            }
            ++at;
            while (at < line.size() && (line[at] == L' ' || line[at] == L'\t'))
            {
                ++at;
            }
            return trim(line.substr(at));
        }

        std::wstring unquote(std::wstring value)
        {
            if (value.size() >= 2 && value.front() == L'"' && value.back() == L'"')
            {
                return value.substr(1, value.size() - 2);
            }
            return value;
        }

        std::wstring configPath()
        {
            PWSTR raw{};
            if (FAILED(SHGetKnownFolderPath(FOLDERID_RoamingAppData, 0, nullptr, &raw)))
            {
                return {};
            }
            std::wstring folder{raw};
            CoTaskMemFree(raw);
            return folder + L"\\" + AppFolder + L"\\" + ConfigFile;
        }

        std::wstring readWholeFile(std::wstring const& path)
        {
            std::ifstream stream(path, std::ios::binary);
            if (!stream)
            {
                return {};
            }
            std::ostringstream buffer;
            buffer << stream.rdbuf();
            std::string narrow = buffer.str();
            if (narrow.empty())
            {
                return {};
            }
            int size = MultiByteToWideChar(CP_UTF8, 0, narrow.data(), static_cast<int>(narrow.size()), nullptr, 0);
            std::wstring wide(static_cast<std::size_t>(size), L'\0');
            MultiByteToWideChar(CP_UTF8, 0, narrow.data(), static_cast<int>(narrow.size()), wide.data(), size);
            return wide;
        }

        std::vector<std::wstring> lines(std::wstring const& text)
        {
            std::vector<std::wstring> out;
            std::size_t start = 0;
            while (start <= text.size())
            {
                auto end = text.find(L'\n', start);
                if (end == std::wstring::npos)
                {
                    end = text.size();
                }
                out.push_back(trim(std::wstring_view{text}.substr(start, end - start)));
                start = end + 1;
            }
            return out;
        }

        struct ControllerConfig
        {
            std::wstring host{L"127.0.0.1"};
            int port{19090};
            std::wstring secret;
        };

        // config.toml is written by the desktop as a flat [mihomo] table for
        // these three keys. A single line scan is enough and avoids taking a
        // TOML dependency into a component that must stay tiny.
        ControllerConfig readControllerConfig(std::wstring const& text)
        {
            ControllerConfig config;
            for (auto const& line : lines(text))
            {
                if (auto v = tomlValue(line, L"controller_host"); !v.empty())
                {
                    config.host = unquote(v);
                }
                if (auto v = tomlValue(line, L"controller_port"); !v.empty())
                {
                    try
                    {
                        config.port = std::stoi(v);
                    }
                    catch (...)
                    {
                    }
                }
                if (auto v = tomlValue(line, L"controller_secret"); !v.empty())
                {
                    config.secret = unquote(v);
                }
            }
            return config;
        }

        // [[clients]] blocks carry `enabled = true/false` and `preset = "..."`.
        // Counting enabled ones and naming the first preset gives the card a
        // useful caption without a full TOML parse.
        std::wstring readRoutingLabel(std::wstring const& text, int& enabledClients)
        {
            int enabled = 0;
            std::wstring firstPreset;
            bool inClients = false;
            for (auto const& line : lines(text))
            {
                if (startsWith(line, L"[[clients]]"))
                {
                    inClients = true;
                    continue;
                }
                if (!line.empty() && line.front() == L'[' && !startsWith(line, L"[["))
                {
                    inClients = false;  // a new table header ends the array
                    continue;
                }
                if (!inClients)
                {
                    continue;
                }
                if (tomlValue(line, L"enabled") == L"true")
                {
                    ++enabled;
                }
                if (firstPreset.empty())
                {
                    if (auto v = tomlValue(line, L"preset"); !v.empty())
                    {
                        firstPreset = unquote(v);
                    }
                }
            }

            enabledClients = enabled;
            if (enabled == 0)
            {
                return L"Direct only";
            }
            if (firstPreset.empty())
            {
                return std::to_wstring(enabled) + L" client(s)";
            }
            return std::to_wstring(enabled) + L" client(s), default " + firstPreset;
        }

        // Pulls a non-negative integer that follows `"key"` in the JSON body.
        bool findNumber(std::wstring const& body, std::wstring_view key, std::uint64_t& out)
        {
            auto at = body.find(key);
            if (at == std::wstring::npos)
            {
                return false;
            }
            at = body.find(L':', at + key.size());
            if (at == std::wstring::npos)
            {
                return false;
            }
            ++at;
            while (at < body.size() && (body[at] == L' ' || body[at] == L'\t'))
            {
                ++at;
            }
            std::uint64_t value = 0;
            bool any = false;
            while (at < body.size() && body[at] >= L'0' && body[at] <= L'9')
            {
                value = value * 10 + static_cast<std::uint64_t>(body[at] - L'0');
                ++at;
                any = true;
            }
            if (any)
            {
                out = value;
            }
            return any;
        }

        // A minimal HTTP/1.1 GET over a raw loopback socket.
        //
        // Deliberately not Windows.Web.Http: that honours the system proxy,
        // and BiFlow itself is the process that installs the tunnel. A raw
        // socket to 127.0.0.1 cannot loop back into our own TUN, and it keeps
        // the provider free of the async HTTP stack entirely.
        struct HttpResult
        {
            bool ok{false};
            int status{0};
            std::wstring body;
            std::wstring error;
        };

        HttpResult httpGet(std::wstring const& host, int port, std::wstring const& secret, std::wstring const& path)
        {
            HttpResult result;
            try
            {
                StreamSocket socket;

                socket.ConnectAsync(winrt::Windows::Networking::HostName{host}, std::to_wstring(port)).get();

                std::wstring request = L"GET " + path + L" HTTP/1.1\r\n";
                request += L"Host: " + host + L"\r\n";
                if (!secret.empty())
                {
                    // Mihomo expects the shared secret verbatim in a Bearer header.
                    request += L"Authorization: Bearer " + secret + L"\r\n";
                }
                request += L"Accept: application/json\r\nConnection: close\r\n\r\n";

                auto writer = DataWriter{socket.OutputStream()};
                writer.WriteString(request);
                writer.StoreAsync().get();
                writer.DetachStream();

                std::string raw;
                DataReader reader{socket.InputStream()};
                for (;;)
                {
                    uint32_t loaded = reader.LoadAsync(2048).get();
                    if (loaded == 0)
                    {
                        break;
                    }
                    std::vector<uint8_t> bytes(loaded);
                    reader.ReadBytes(bytes);
                    raw.append(reinterpret_cast<char const*>(bytes.data()), loaded);
                    if (raw.size() > (4u << 20))  // a /connections body is far smaller
                    {
                        break;
                    }
                }
                socket.Close();

                int size = MultiByteToWideChar(CP_UTF8, 0, raw.data(), static_cast<int>(raw.size()), nullptr, 0);
                std::wstring wide(static_cast<std::size_t>(size), L'\0');
                if (size > 0)
                {
                    MultiByteToWideChar(CP_UTF8, 0, raw.data(), static_cast<int>(raw.size()), wide.data(), size);
                }

                auto split = wide.find(L"\r\n\r\n");
                if (split == std::wstring::npos)
                {
                    result.error = L"controller sent no complete response";
                    return result;
                }
                result.body = wide.substr(split + 4);

                auto space = wide.find(L' ');
                if (space != std::wstring::npos)
                {
                    result.status = std::stoi(wide.substr(space + 1, 3));
                }
                result.ok = result.status == 200;
                return result;
            }
            catch (hresult_error const& error)
            {
                result.error = L"controller offline";
                (void)error;
                return result;
            }
            catch (...)
            {
                result.error = L"controller offline";
                return result;
            }
        }

        std::wstring jsonEscape(std::wstring_view text)
        {
            std::wstring out;
            out.reserve(text.size() + 8);
            for (wchar_t c : text)
            {
                switch (c)
                {
                case L'"': out += L"\\\""; break;
                case L'\\': out += L"\\\\"; break;
                case L'\n': out += L"\\n"; break;
                case L'\r': out += L"\\r"; break;
                case L'\t': out += L"\\t"; break;
                default:
                    if (c < 0x20)
                    {
                        wchar_t buffer[8];
                        swprintf_s(buffer, L"\\u%04x", static_cast<unsigned>(c));
                        out += buffer;
                    }
                    else
                    {
                        out += c;
                    }
                }
            }
            return out;
        }

        std::wstring textBlock(std::wstring_view text, std::wstring_view size, std::wstring_view extra)
        {
            // Built by appending rather than by `a + b + c`: there is no
            // operator+ between a wchar_t literal and a wstring_view, and
            // chaining through hstring would drag the whole string into a
            // WinRT type for no reason.
            std::wstring out;
            out.reserve(text.size() + size.size() + extra.size() + 48);
            out += L"{\"type\":\"TextBlock\",\"text\":\"";
            out += jsonEscape(text);
            out += L"\",\"size\":\"";
            out += size;
            out += L"\"";
            out += extra;
            out += L"}";
            return out;
        }

        std::wstring metricColumn(std::wstring_view label, std::uint64_t bytes, bool downloadAccent)
        {
            // The color belongs *inside* the TextBlock object. textBlock()
            // closes the object it builds, so appending ,"color":"accent"
            // after the call emits `"color":}"accent"` - invalid JSON, which
            // the host answers by showing its placeholder skeleton forever
            // instead of the card.
            std::wstring valueExtra = L",\"weight\":\"bolder\",\"wrap\":true,\"spacing\":\"none\",\"color\":\"";
            valueExtra += downloadAccent ? L"accent" : L"warning";
            valueExtra += L"\"";

            // Cyan for download, amber for upload - the same two hues the
            // desktop used for its live-connection view, so the widget and
            // the app read as one product.
            std::wstring out;
            out += L"{\"type\":\"Column\",\"width\":\"stretch\",\"items\":[";
            out += textBlock(label, L"small", L",\"isSubtle\":true,\"spacing\":\"none\"");
            out += L",";
            out += textBlock(formatBytes(bytes), L"large", valueExtra);
            out += L"]}";
            return out;
        }
    }

    std::wstring formatBytes(std::uint64_t bytes)
    {
        static const wchar_t* units[] = {L"B", L"KiB", L"MiB", L"GiB", L"TiB", L"PiB"};
        double value = static_cast<double>(bytes);
        int unit = 0;
        while (value >= 1024.0 && unit < 5)
        {
            value /= 1024.0;
            ++unit;
        }
        wchar_t buffer[64];
        if (unit == 0)
        {
            swprintf_s(buffer, L"%lld %s", static_cast<long long>(bytes), units[unit]);
        }
        else
        {
            swprintf_s(buffer, L"%.2f %s", value, units[unit]);
        }
        return buffer;
    }

    Snapshot readSnapshot()
    {
        auto path = configPath();
        if (path.empty())
        {
            return Snapshot::unreachable(L"Roaming app data unavailable");
        }
        auto text = readWholeFile(path);
        if (text.empty())
        {
            return Snapshot::unreachable(L"config.toml not found");
        }

        auto config = readControllerConfig(text);

        Snapshot snapshot;
        int enabledClients = 0;
        snapshot.defaultRouteLabel = readRoutingLabel(text, enabledClients);
        snapshot.enabledClients = enabledClients;

        auto response = httpGet(config.host, config.port, config.secret, L"/connections");
        if (!response.ok)
        {
            snapshot.failureReason = response.status == 0
                ? (response.error.empty() ? std::wstring{L"controller offline"} : response.error)
                : L"controller HTTP " + std::to_wstring(response.status);
            return snapshot;
        }

        // The controller only answers while Mihomo runs, so a successful read
        // IS the "connected" signal. There is no separate stack phase to read
        // without launching the desktop.
        snapshot.controllerReachable = true;
        snapshot.connected = true;
        findNumber(response.body, L"\"uploadTotal\"", snapshot.uploadedTotal);
        findNumber(response.body, L"\"downloadTotal\"", snapshot.downloadedTotal);
        return snapshot;
    }

    std::wstring buildCard(Snapshot const& snapshot, bool withTotals)
    {
        std::wstring headline = snapshot.connected ? L"Split active"
                           : snapshot.controllerReachable ? L"Idle"
                                                           : L"Disconnected";
        std::wstring subtitle;
        if (snapshot.connected)
        {
            subtitle = snapshot.enabledClients > 0
                ? L"Routing via " + snapshot.defaultRouteLabel
                : L"Iran direct, all other traffic direct";
        }
        else
        {
            subtitle = snapshot.failureReason.empty() ? L"Mihomo is not running" : snapshot.failureReason;
        }

        std::wstring body;
        body += textBlock(headline, L"extraLarge", L",\"weight\":\"bolder\",\"wrap\":true,\"spacing\":\"none\"");
        body += L",";
        body += textBlock(subtitle, L"small", L",\"isSubtle\":true,\"spacing\":\"none\",\"maxLines\":2,\"wrap\":true");

        if (withTotals)
        {
            body += L",";
            body += textBlock(L" ", L"small", L",\"spacing\":\"none\"");
            body += L",";
            body += L"{\"type\":\"ColumnSet\",\"columns\":[" +
                    metricColumn(L"Download", snapshot.downloadedTotal, true) + L"," +
                    metricColumn(L"Upload", snapshot.uploadedTotal, false) + L"]}";
            body += L",";
            body += textBlock(snapshot.defaultRouteLabel, L"small",
                              L",\"isSubtle\":true,\"spacing\":\"small\",\"wrap\":true,\"maxLines\":1");
        }
        else
        {
            // Small card: one combined line, no column set.
            body += L",";
            // U+2193 / U+2191 written as escapes, not the three UTF-8 bytes:
            // L"\xE2\x86\x93" is U+00E2 U+0086 U+0093 and renders as "â†“".
            body += textBlock(L"↓ " + formatBytes(snapshot.downloadedTotal) + L"   ↑ " +
                                  formatBytes(snapshot.uploadedTotal),
                              L"medium", L",\"wrap\":true,\"spacing\":\"small\",\"maxLines\":2");
        }

        // The widget host's Adaptive Card renderer accepts 1.0 to 1.5 and
        // refuses a higher declared version outright - it then shows its
        // placeholder skeleton and never renders the body. Every enum here is
        // camelCase for the same reason: an unrecognised value is dropped, and
        // a TextBlock with no recognised size does not lay out.
        return L"{\"type\":\"AdaptiveCard\",\"$schema\":\"http://adaptivecards.io/schemas/adaptive-card.json\","
               L"\"version\":\"1.0\",\"body\":[" + body + L"]}";
    }
}
