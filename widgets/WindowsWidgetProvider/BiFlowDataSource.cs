using System.Net.Http.Headers;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace BiFlow.WidgetProvider;

/// <summary>Everything the widget card needs for one refresh.</summary>
public sealed record WidgetSnapshot(
    bool ControllerReachable,
    bool Connected,
    ulong UploadedTotal,
    ulong DownloadedTotal,
    string DefaultRouteLabel,
    int EnabledClients,
    string? FailureReason)
{
    public static WidgetSnapshot Unreachable(string reason) =>
        new(false, false, 0, 0, "—", 0, reason);
}

/// <summary>
/// Reads the same Mihomo REST controller the BiFlow desktop samples.
/// <para>
/// The widget deliberately does NOT read the desktop's in-memory
/// accumulator and does NOT parse the 250 MB+ <c>debug.log</c>: the
/// controller is already the authoritative, live source, and reaching
/// it directly means the Windows 11 widget needs ZERO changes to the
/// existing Tauri application (ADR 0111 keeps session totals in the
/// desktop process on purpose).
/// </para>
/// </summary>
public sealed class BiFlowDataSource
{
    private const string ConfigFileName = "config.toml";
    private const string AppFolderName = "biflow";

    private static readonly TimeSpan ProbeTimeout = TimeSpan.FromSeconds(2);

    private readonly HttpClient _http;

    public BiFlowDataSource()
    {
        _http = new HttpClient
        {
            // Loopback only. Mihomo's controller listens on 127.0.0.1 and
            // is reached directly - the request must not inherit the
            // system proxy or the running Hiddify tunnel, or it would
            // loop back into our own TUN.
            Timeout = ProbeTimeout,
        };
        _http.DefaultRequestHeaders.Accept.Add(
            new MediaTypeWithQualityHeaderValue("application/json"));
    }

    /// <summary>Collects one snapshot. Never throws.</summary>
    public async Task<WidgetSnapshot> ReadAsync(CancellationToken cancellationToken = default)
    {
        ControllerConfig controller;
        RoutingSummary routing;
        try
        {
            controller = ControllerConfig.Load();
            routing = RoutingSummary.Load();
        }
        catch (Exception ex)
        {
            return WidgetSnapshot.Unreachable($"config unreadable: {ex.Message}");
        }

        try
        {
            using var request = new HttpRequestMessage(
                HttpMethod.Get,
                $"http://{controller.Host}:{controller.Port}/connections");
            // Mihomo expects the shared secret verbatim in a Bearer header.
            request.Headers.Authorization =
                new AuthenticationHeaderValue("Bearer", controller.Secret);

            using var response = await _http
                .SendAsync(request, HttpCompletionOption.ResponseContentRead, cancellationToken)
                .ConfigureAwait(false);

            if (!response.IsSuccessStatusCode)
            {
                return WidgetSnapshot.Unreachable(
                    $"controller HTTP {(int)response.StatusCode}");
            }

            await using var stream = await response.Content
                .ReadAsStreamAsync(cancellationToken)
                .ConfigureAwait(false);

            var payload = await JsonSerializer
                .DeserializeAsync<ConnectionsResponse>(stream, JsonOptions, cancellationToken)
                .ConfigureAwait(false);

            if (payload is null)
            {
                return WidgetSnapshot.Unreachable("controller returned no JSON");
            }

            // The controller only answers while Mihomo is running, so a
            // successful read IS the "connected" signal. There is no
            // separate stack phase to read without launching the desktop.
            return new WidgetSnapshot(
                ControllerReachable: true,
                Connected: true,
                UploadedTotal: payload.UploadTotal ?? 0,
                DownloadedTotal: payload.DownloadTotal ?? 0,
                DefaultRouteLabel: routing.DefaultRouteLabel,
                EnabledClients: routing.EnabledClients,
                FailureReason: null);
        }
        catch (TaskCanceledException) when (!cancellationToken.IsCancellationRequested)
        {
            return WidgetSnapshot.Unreachable("controller timed out");
        }
        catch (HttpRequestException ex)
        {
            return WidgetSnapshot.Unreachable($"controller offline ({ex.GetBaseException().Message})");
        }
        catch (JsonException ex)
        {
            return WidgetSnapshot.Unreachable($"controller JSON invalid ({ex.Message})");
        }
    }

    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNameCaseInsensitive = true,
        NumberHandling = JsonNumberHandling.AllowReadingFromString,
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
    };

    private sealed class ConnectionsResponse
    {
        [JsonPropertyName("uploadTotal")]
        public ulong? UploadTotal { get; init; }

        [JsonPropertyName("downloadTotal")]
        public ulong? DownloadTotal { get; init; }
    }

    /// <summary>Endpoint of the local Mihomo controller.</summary>
    private sealed record ControllerConfig(string Host, int Port, string Secret)
    {
        public static ControllerConfig Load()
        {
            var text = File.ReadAllText(ConfigPath);
            var host = ReadString(text, "controller_host") ?? "127.0.0.1";
            var port = ReadInt(text, "controller_port") ?? 19090;
            var secret = ReadString(text, "controller_secret") ?? string.Empty;
            return new ControllerConfig(host, port, secret);
        }

        private static string ConfigPath => Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData),
            AppFolderName,
            ConfigFileName);

        // config.toml is written by the desktop as a flat `[mihomo]` table
        // for these three keys. A single line scan is enough and avoids
        // taking a TOML dependency into a component that must stay tiny.
        private static string? ReadString(string text, string key)
        {
            foreach (var raw in text.Split('\n'))
            {
                var line = raw.Trim();
                if (!line.StartsWith(key + " =", StringComparison.Ordinal))
                {
                    continue;
                }
                var value = line[(key.Length + 3)..].Trim();
                if (value.Length >= 2 && value[0] == '"' && value[^1] == '"')
                {
                    return value[1..^1];
                }
            }
            return null;
        }

        private static int? ReadInt(string text, string key)
        {
            foreach (var raw in text.Split('\n'))
            {
                var line = raw.Trim();
                if (!line.StartsWith(key + " =", StringComparison.Ordinal))
                {
                    continue;
                }
                var value = line[(key.Length + 3)..].Trim();
                if (int.TryParse(value, out var parsed))
                {
                    return parsed;
                }
            }
            return null;
        }
    }

    /// <summary>Best-effort routing summary shown as the widget subtitle.</summary>
    private sealed record RoutingSummary(string DefaultRouteLabel, int EnabledClients)
    {
        public static RoutingSummary Load()
        {
            var path = Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData),
                AppFolderName,
                ConfigFileName);
            if (!File.Exists(path))
            {
                return new RoutingSummary("—", 0);
            }

            var text = File.ReadAllText(path);

            // `[[clients]]` blocks each carry `enabled = true/false` and
            // `preset = "<name>"`. Counting the enabled ones and naming
            // the first preset gives the card a useful caption without
            // needing a full TOML parse.
            var enabled = 0;
            string? firstPreset = null;
            var inClients = false;
            foreach (var raw in text.Split('\n'))
            {
                var line = raw.Trim();
                if (line.StartsWith("[[clients]]", StringComparison.Ordinal))
                {
                    inClients = true;
                    continue;
                }
                // A new table header ends the clients array.
                if (line.StartsWith('[') && !line.StartsWith("[[", StringComparison.Ordinal))
                {
                    inClients = false;
                    continue;
                }
                if (!inClients)
                {
                    continue;
                }
                if (line.StartsWith("enabled = true", StringComparison.Ordinal))
                {
                    enabled++;
                }
                if (firstPreset is null && line.StartsWith("preset = ", StringComparison.Ordinal))
                {
                    firstPreset = line["preset = ".Length..].Trim().Trim('"');
                }
            }

            var label = enabled == 0
                ? "Direct only"
                : string.IsNullOrEmpty(firstPreset)
                    ? $"{enabled} client(s)"
                    : $"{enabled} client(s), default {firstPreset}";

            return new RoutingSummary(label, enabled);
        }
    }
}
