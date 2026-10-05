using System.Text.Json;
using System.Text.Json.Nodes;

namespace BiFlow.WidgetProvider;

/// <summary>
/// Builds the Adaptive Card JSON the Widgets Board renders.
/// <para>
/// Adaptive Cards is the only templating format the Windows widget host
/// accepts for a native (non-web) widget, so the card is assembled as a
/// JSON tree rather than XAML. Two sizes are emitted from the same
/// builder: the <c>medium</c> card is the default, the <c>small</c> card
/// drops the routing caption so the totals stay legible.
/// </para>
/// </summary>
public static class AdaptiveCardBuilder
{
    /// <summary>Widget sizes this provider registers in its manifest.</summary>
    public enum CardSize
    {
        Small,
        Medium,
    }

    public static string Build(WidgetSnapshot snapshot, CardSize size)
    {
        var body = new JsonArray
        {
            // ---- headline: stack status -------------------------------
            new JsonObject
            {
                ["type"] = "TextBlock",
                ["text"] = StatusText(snapshot),
                ["size"] = "ExtraLarge",
                ["weight"] = "Bolder",
                ["wrap"] = true,
                ["spacing"] = "None",
            },
            new JsonObject
            {
                ["type"] = "TextBlock",
                ["text"] = StatusSubtitle(snapshot),
                ["size"] = "Small",
                ["isSubtle"] = true,
                ["spacing"] = "None",
                ["maxLines"] = 2,
                ["wrap"] = true,
            },
        };

        if (size == CardSize.Medium)
        {
            // ---- separator --------------------------------------------
            body.Add(new JsonObject
            {
                ["type"] = "TextBlock",
                ["text"] = " ",
                ["size"] = "Small",
                ["spacing"] = "None",
            });

            // ---- two columns: download | upload ------------------------
            body.Add(new JsonObject
            {
                ["type"] = "ColumnSet",
                ["columns"] = new JsonArray
                {
                    MetricColumn("Download", snapshot.DownloadedTotal, downloadAccent: true),
                    MetricColumn("Upload", snapshot.UploadedTotal, downloadAccent: false),
                },
            });

            // ---- routing caption --------------------------------------
            body.Add(new JsonObject
            {
                ["type"] = "TextBlock",
                ["text"] = snapshot.DefaultRouteLabel,
                ["size"] = "Small",
                ["isSubtle"] = true,
                ["spacing"] = "Small",
                ["wrap"] = true,
                ["maxLines"] = 1,
            });
        }
        else
        {
            // Small card: one combined line, no column set.
            body.Add(new JsonObject
            {
                ["type"] = "TextBlock",
                ["text"] = $"↓ {FormatBytes(snapshot.DownloadedTotal)}   ↑ {FormatBytes(snapshot.UploadedTotal)}",
                ["size"] = "Medium",
                ["wrap"] = true,
                ["spacing"] = "Small",
                ["maxLines"] = 2,
            });
        }

        return new JsonObject
        {
            ["type"] = "AdaptiveCard",
            ["$schema"] = "http://adaptivecards.io/schemas/adaptive-card.json",
            ["version"] = "1.6",
            ["body"] = body,
        }.ToJsonString(new JsonSerializerOptions
        {
            WriteIndented = false,
        });
    }

    private static JsonObject MetricColumn(string label, ulong bytes, bool downloadAccent) =>
        new()
        {
            ["type"] = "Column",
            ["width"] = "stretch",
            ["items"] = new JsonArray
            {
                new JsonObject
                {
                    ["type"] = "TextBlock",
                    ["text"] = label,
                    ["size"] = "Caption",
                    ["isSubtle"] = true,
                    ["spacing"] = "None",
                },
                new JsonObject
                {
                    ["type"] = "TextBlock",
                    ["text"] = FormatBytes(bytes),
                    ["size"] = "Large",
                    ["weight"] = "Bolder",
                    ["wrap"] = true,
                    ["spacing"] = "None",
                    // Cyan for download, amber for upload - the same two
                    // hues the desktop used for its live-connection view,
                    // so the widget and the app read as one product.
                    ["color"] = downloadAccent ? "Accent" : "Warning",
                },
            },
        };

    private static string StatusText(WidgetSnapshot snapshot)
    {
        if (snapshot.Connected)
        {
            return "Split active";
        }

        if (snapshot.ControllerReachable)
        {
            return "Idle";
        }

        return "Disconnected";
    }

    private static string StatusSubtitle(WidgetSnapshot snapshot)
    {
        if (snapshot.Connected)
        {
            return snapshot.EnabledClients > 0
                ? $"Routing via {snapshot.DefaultRouteLabel}"
                : "Iran direct, all other traffic direct";
        }

        return snapshot.FailureReason ?? "Mihomo is not running";
    }

    /// <summary>
    /// Binary units, matching the desktop's status bar so the same number
    /// never appears with two different suffixes.
    /// </summary>
    internal static string FormatBytes(ulong bytes)
    {
        string[] units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
        double value = bytes;
        var unit = 0;
        while (value >= 1024d && unit < units.Length - 1)
        {
            value /= 1024d;
            unit++;
        }

        return unit == 0
            ? $"{(long)value} {units[unit]}"
            : $"{value:0.00} {units[unit]}";
    }
}
