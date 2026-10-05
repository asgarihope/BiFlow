using System.Runtime.InteropServices;
using Microsoft.Windows.Widgets.Providers;

namespace BiFlow.WidgetProvider;

/// <summary>
/// The Widgets Board calls this class through COM. Every member here is a
/// host callback: the host owns the lifetime, we only answer.
/// <para>
/// There is no UI thread, no window, and no message pump in this class.
/// The process is a bare COM server that the widget host starts, calls
/// once per lifecycle event, and lets exit.
/// </para>
/// </summary>
[ComVisible(true)]
[ComExport]
[Guid(WidgetIds.ProviderClsid)]
public sealed class WidgetProvider : IWidgetProvider
{
    private readonly BiFlowDataSource _data = new();

    /// <summary>
    /// The host asks for fresh content. This is the only method that
    /// matters: the board calls it when a widget is first pinned, when it
    /// becomes visible, and on its own refresh cadence.
    /// </summary>
    public void Update(WidgetUpdateRequest updateRequest)
    {
        if (updateRequest is null)
        {
            return;
        }

        var targets = CollectWidgetIds(updateRequest.WidgetIds);
        if (targets.Count == 0)
        {
            return;
        }

        var card = RenderCard();

        using var manager = new WidgetManager(WidgetIds.ProviderClsid);
        foreach (var id in targets)
        {
            manager.UpdateWidget(
                new WidgetInfo
                {
                    Template = new WidgetTemplate(card),
                    Data = card,
                    IsPlaceholderContent = false,
                    LastUpdated = DateTimeOffset.UtcNow,
                },
                new WidgetUpdateRequestOptions(id, card));
        }
    }

    /// <summary>
    /// The host changed a widget's context (theme, host surface, size).
    /// The card is theme-neutral - the host applies light and dark itself
    /// - so a re-render is enough to repaint into the new container.
    /// </summary>
    public void OnWidgetContextChanged(WidgetContextChangedArgs contextChangedArgs)
    {
        var targets = CollectWidgetIds(
            contextChangedArgs?.WidgetContexts?.Keys);
        if (targets.Count == 0)
        {
            return;
        }

        var card = RenderCard();

        using var manager = new WidgetManager(WidgetIds.ProviderClsid);
        foreach (var id in targets)
        {
            manager.UpdateWidget(
                new WidgetInfo
                {
                    Template = new WidgetTemplate(card),
                    Data = card,
                    IsPlaceholderContent = false,
                    LastUpdated = DateTimeOffset.UtcNow,
                },
                new WidgetUpdateRequestOptions(id, card));
        }
    }

    /// <summary>Required by the interface; the card has no actions.</summary>
    public void OnActionInvoked(WidgetActionInvokedArgs actionInvokedArgs)
    {
    }

    /// <summary>
    /// Required by the interface. Deliberately silent: this process has no
    /// user-facing surface and no log consumer. A controller failure is
    /// reported through the card itself, which then reads "Disconnected"
    /// with the reason underneath.
    /// </summary>
    public void OnErrorInfoReported(WidgetErrorInfoReportedArgs errorInfoArgs)
    {
    }

    /// <summary>Required by the interface; BiFlow collects no telemetry.</summary>
    public void OnAnalyticsInfoReported(WidgetAnalyticsInfoReportedArgs analyticsInfoArgs)
    {
    }

    /// <summary>
    /// Adaptive Cards scale themselves across the small/medium/large
    /// surfaces, so one template serves every size the host asks for.
    /// Keeping a single template is deliberate: it removes the
    /// <c>WidgetSize</c> struct comparison that otherwise has to be
    /// rebuilt for each new Windows App SDK projection.
    /// </summary>
    private string RenderCard()
    {
        var snapshot = _data.ReadAsync().GetAwaiter().GetResult();
        return AdaptiveCardBuilder.Build(snapshot, AdaptiveCardBuilder.CardSize.Medium);
    }

    private static List<string> CollectWidgetIds(IList<string>? source)
    {
        var ids = new List<string>();
        if (source is null)
        {
            return ids;
        }

        foreach (var id in source)
        {
            if (!string.IsNullOrWhiteSpace(id))
            {
                ids.Add(id);
            }
        }

        return ids;
    }
}
