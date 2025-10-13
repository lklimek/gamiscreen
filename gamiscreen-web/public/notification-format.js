(function (global) {
  function formatRemaining(event) {
    const minutes = typeof event.remaining_minutes === 'number' ? event.remaining_minutes : null;
    const name = event.display_name || event.child_id || 'child';
    if (minutes == null) {
      return {
        title: 'Gamiscreen',
        body: `Remaining time updated for ${name}.`,
        url: event.child_id ? `#child/${encodeURIComponent(event.child_id)}` : '#status',
      };
    }
    if (minutes <= 0) {
      return {
        title: 'Gamiscreen',
        body: `0 minutes remaining — ${name} is out of time.`,
        url: event.child_id ? `#child/${encodeURIComponent(event.child_id)}` : '#status',
      };
    }
    const desc = minutes === 1 ? '1 minute' : `${minutes} minutes`;
    return {
      title: 'Gamiscreen',
      body: `${desc} remaining — ${name}.`,
      url: event.child_id ? `#child/${encodeURIComponent(event.child_id)}` : '#status',
    };
  }

  function formatPending(event) {
    const count = typeof event.count === 'number' ? event.count : 0;
    const body =
      count > 0
        ? `${count} notification${count === 1 ? '' : 's'} pending.`
        : 'All notifications resolved.';
    return {
      title: 'Gamiscreen',
      body,
      url: '#notifications',
    };
  }

  function formatNotification(event) {
    if (!event || typeof event !== 'object') return null;
    const type = event.type;
    if (type === 'remaining_updated') {
      return formatRemaining(event);
    }
    if (type === 'pending_count') {
      return formatPending(event);
    }
    if (event.title || event.body) {
      return {
        title: event.title || 'Gamiscreen',
        body: event.body || '',
        url: event.url || '#status',
      };
    }
    return null;
  }

  global.__gamiscreenFormatNotification = formatNotification;
})(typeof self !== 'undefined' ? self : window);

