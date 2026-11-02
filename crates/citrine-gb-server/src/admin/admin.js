(() => {
  const units = ["B", "KB", "MB", "GB", "TB"];
  const bytes = (n) => {
    let i = 0;
    while (n >= 1024 && i < units.length - 1) { n /= 1024; i++; }
    return (i === 0 || n >= 100 ? Math.round(n) : n.toFixed(1)) + " " + units[i];
  };
  const duration = (s) => {
    const d = Math.floor(s / 86400), h = Math.floor(s % 86400 / 3600), m = Math.floor(s % 3600 / 60);
    if (d > 0) return d + " d " + h + " h";
    if (h > 0) return h + " h " + m + " min";
    if (m > 0) return m + " min";
    return Math.floor(s) + " s";
  };
  const set = (key, text) => document.querySelectorAll('[data-stat="' + key + '"]').forEach((el) => { el.textContent = text; });

  const uptimeEl = document.querySelector('[data-stat="uptime"]');
  let uptime = uptimeEl ? Number(uptimeEl.dataset.seconds) : 0;
  let base = performance.now();
  if (uptimeEl) setInterval(() => set("uptime", duration(uptime + (performance.now() - base) / 1000)), 1000);

  if (document.querySelector("[data-live]")) {
    const poll = async () => {
      try {
        const r = await fetch("/admin/stats", { credentials: "same-origin" });
        if (!r.ok) return;
        const s = await r.json();
        set("online", s.online);
        set("peak", s.peak_today);
        set("accounts", s.accounts);
        set("sessions", s.sessions);
        set("messages", s.messages_today);
        set("storage", bytes(s.storage_used));
        set("storage_total", "of " + bytes(s.storage_total));
        set("bytes_in", bytes(s.bytes_in));
        set("bytes_out", bytes(s.bytes_out));
        set("database", bytes(s.database));
        set("waiting", s.waiting);
        set("waiting_bytes", bytes(s.waiting_bytes));
        Object.entries(s.refused || {}).forEach(([key, n]) => {
          set("refused_" + key, n);
          document.querySelectorAll('[data-refused="' + key + '"]').forEach((row) => row.classList.toggle("none", n === 0));
        });
        const bar = document.querySelector('[data-stat="storage_bar"]');
        if (bar) bar.style.width = (s.storage_total ? Math.min(100, s.storage_used / s.storage_total * 100) : 0) + "%";
        uptime = s.uptime; base = performance.now();
      } catch (_) {}
    };
    setInterval(poll, 5000);
  }

  document.querySelectorAll("[data-copy]").forEach((btn) => {
    btn.addEventListener("click", async () => {
      try {
        await navigator.clipboard.writeText(btn.dataset.copy);
        const label = btn.querySelector("span");
        if (label) { const old = label.textContent; label.textContent = "Copied"; setTimeout(() => { label.textContent = old; }, 1500); }
      } catch (_) {}
    });
  });

  const esc = document.querySelector("[data-esc]");
  if (esc) document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && !document.querySelector(":popover-open")) location.href = esc.href;
  });

  document.querySelectorAll(".hint[popovertarget]").forEach((btn) => {
    const pop = document.getElementById(btn.getAttribute("popovertarget"));
    if (!pop || !pop.showPopover) return;
    const place = () => {
      const r = btn.getBoundingClientRect();
      pop.style.margin = "0";
      pop.style.right = "auto";
      pop.style.bottom = "auto";
      pop.style.left = Math.max(16, Math.min(r.left, window.innerWidth - pop.offsetWidth - 16)) + "px";
      pop.style.top = ((btn.closest(".field-row") || btn).getBoundingClientRect().bottom + 8) + "px";
    };
    let pinned = false;
    let pinnedAtPress = null;
    pop.addEventListener("toggle", (e) => { if (e.newState === "open") place(); else pinned = false; });
    btn.addEventListener("pointerdown", () => { pinnedAtPress = pinned; });
    btn.addEventListener("click", (e) => {
      e.preventDefault();
      const wasPinned = pinnedAtPress ?? pinned;
      pinnedAtPress = null;
      if (wasPinned) {
        pinned = false;
        if (pop.matches(":popover-open")) pop.hidePopover();
      } else {
        if (!pop.matches(":popover-open")) pop.showPopover();
        pinned = true;
      }
    });
    btn.addEventListener("pointerenter", (e) => { if (e.pointerType === "mouse" && !pop.matches(":popover-open")) pop.showPopover(); });
    btn.addEventListener("pointerleave", (e) => { if (e.pointerType === "mouse" && !pinned && pop.matches(":popover-open")) pop.hidePopover(); });
  });
})();
