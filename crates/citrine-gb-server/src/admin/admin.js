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

  document.querySelectorAll("[popovertarget][data-anchor]").forEach((btn) => {
    const pop = document.getElementById(btn.getAttribute("popovertarget"));
    if (!pop || !pop.showPopover) return;
    pop.addEventListener("toggle", (e) => {
      if (e.newState !== "open") return;
      const r = btn.getBoundingClientRect();
      pop.style.margin = "0";
      pop.style.right = "auto";
      pop.style.bottom = "auto";
      const left = btn.dataset.anchor === "end" ? r.right - pop.offsetWidth : r.left;
      pop.style.left = Math.max(16, Math.min(left, window.innerWidth - pop.offsetWidth - 16)) + "px";
      pop.style.top = (r.bottom + 6) + "px";
    });
  });

  if (document.querySelector(".game-row, #adding-row") && document.body.showPopover !== undefined) {
    const pop = document.createElement("div");
    pop.className = "hint-pop above";
    pop.setAttribute("popover", "auto");
    document.body.appendChild(pop);
    let shownFor = null;
    let openAtPress = null;
    pop.addEventListener("toggle", (e) => { if (e.newState !== "open") shownFor = null; });
    const titleOf = (e) => e.target.closest && e.target.closest(".game-row .title");
    document.addEventListener("pointerdown", (e) => { if (titleOf(e)) openAtPress = shownFor; });
    document.addEventListener("click", (e) => {
      const title = titleOf(e);
      if (!title) return;
      const was = openAtPress ?? shownFor;
      openAtPress = null;
      if (was === title) {
        if (pop.matches(":popover-open")) pop.hidePopover();
        return;
      }
      if (title.scrollWidth <= title.clientWidth) return;
      pop.textContent = title.textContent;
      pop.style.margin = "0";
      pop.style.right = "auto";
      pop.style.bottom = "auto";
      pop.style.width = "max-content";
      pop.style.maxWidth = (window.innerWidth - 32) + "px";
      if (!pop.matches(":popover-open")) pop.showPopover();
      shownFor = title;
      const r = title.getBoundingClientRect();
      const mid = r.left + r.width / 2;
      const left = Math.max(16, Math.min(mid - pop.offsetWidth / 2, window.innerWidth - pop.offsetWidth - 16));
      pop.style.left = left + "px";
      pop.style.top = (r.top - 9 - pop.offsetHeight) + "px";
      pop.style.setProperty("--arrow-x", (mid - left) + "px");
    });
  }

  document.querySelectorAll(".hint[popovertarget]").forEach((btn) => {
    const pop = document.getElementById(btn.getAttribute("popovertarget"));
    if (!pop || !pop.showPopover) return;
    const place = () => {
      const r = btn.getBoundingClientRect();
      pop.style.margin = "0";
      pop.style.right = "auto";
      pop.style.bottom = "auto";
      pop.style.width = "";
      pop.style.maxWidth = "";
      pop.classList.remove("above");
      if (btn.closest(".user-row")) {
        pop.style.width = "max-content";
        pop.style.maxWidth = (window.innerWidth - 32) + "px";
        pop.classList.add("above");
        const mid = r.left + r.width / 2;
        const left = Math.max(16, Math.min(mid - pop.offsetWidth / 2, window.innerWidth - pop.offsetWidth - 16));
        pop.style.left = left + "px";
        const icon = btn.querySelector("svg");
        const top = icon ? icon.getBoundingClientRect().top : r.top;
        pop.style.top = (top - 9 - pop.offsetHeight) + "px";
        pop.style.setProperty("--arrow-x", (mid - left) + "px");
      } else {
        pop.style.left = Math.max(16, Math.min(r.left, window.innerWidth - pop.offsetWidth - 16)) + "px";
        pop.style.top = ((btn.closest(".field-row") || btn).getBoundingClientRect().bottom + 8) + "px";
      }
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

  const adder = document.querySelector("[data-add-games]");
  if (adder) {
    const picker = document.getElementById("add-games-file");
    const card = document.querySelector(".games");
    const template = document.getElementById("adding-row");
    const consoles = JSON.parse(adder.dataset.consoles);
    const limit = Number(adder.dataset.fileBytes);
    const queue = [];
    const rows = [];
    const tally = { added: 0, there: 0, failed: 0 };
    const keep = new URLSearchParams({ done: "upload" });
    let running = null;
    const ofBytes = (done, of) => {
      let i = 0, d = 1;
      while (of / d >= 1024 && i < units.length - 1) { d *= 1024; i++; }
      const whole = i === 0 || of / d >= 100;
      const f = (n) => (whole ? Math.round(n / d) : (n / d).toFixed(1));
      return f(done) + " of " + f(of) + " " + units[i];
    };
    const show = (item, words, done, of) => {
      item.words.textContent = of === undefined ? words : words + " " + ofBytes(done, of);
      item.bar.style.width = (of ? Math.min(100, done / of * 100) : 0) + "%";
    };
    const empty = () => {
      const text = card.querySelector(".empty");
      if (text) text.hidden = rows.length > 0;
    };
    const finish = () => {
      if (running || queue.length || tally.added + tally.there + tally.failed === 0) return;
      keep.set("added", tally.added);
      keep.set("there", tally.there);
      keep.set("failed", tally.failed);
      keep.set("order", "newest");
      location.href = "/admin/archive?" + keep.toString();
    };
    const settle = (item, last) => {
      item.row.classList.remove("busy");
      item.row.querySelector("[data-cancel]").remove();
      item.bar.parentElement.remove();
      if (last && (last.step === "added" || last.step === "there")) {
        tally[last.step]++;
        if (!keep.has(last.step + "_game")) keep.set(last.step + "_game", last.id);
        item.words.textContent = last.step === "added" ? "Added" : "Already in the Game Archive";
        if (last.name) {
          const title = item.row.querySelector(".title");
          title.textContent = last.name;
          title.title = last.name;
        }
        if (last.cover) {
          const cover = document.createElement("img");
          cover.className = "cover";
          cover.alt = "";
          cover.width = 40;
          cover.height = 40;
          cover.src = "/admin/archive/" + last.id + "/cover";
          item.row.querySelector(".cover").replaceWith(cover);
        }
        return;
      }
      tally.failed++;
      if (!keep.has("why")) {
        keep.set("why", last && last.why ? last.why : "broken");
        keep.set("failed_file", item.file.name);
      }
      item.row.classList.add("failed");
      item.words.textContent = "Not added";
    };
    const send = (item) => new Promise((resolve) => {
      const xhr = new XMLHttpRequest();
      let seen = 0, last = null;
      item.xhr = xhr;
      const read = () => {
        const text = xhr.responseText;
        let nl;
        while ((nl = text.indexOf("\n", seen)) >= 0) {
          let msg = null;
          try { msg = JSON.parse(text.slice(seen, nl)); } catch (_) {}
          seen = nl + 1;
          if (!msg) continue;
          if (msg.step === "compressing") show(item, "Compressing", msg.done, msg.of);
          else if (msg.step === "cover") { show(item, "Finding the cover"); item.bar.style.width = "100%"; }
          else last = msg;
        }
      };
      xhr.open("POST", "/admin/archive/upload?name=" + encodeURIComponent(item.file.name));
      xhr.upload.onprogress = (e) => show(item, "Uploading", e.loaded, item.file.size);
      xhr.onprogress = read;
      xhr.onload = read;
      xhr.onloadend = () => { if (!item.cancelled) settle(item, last); resolve(); };
      show(item, "Uploading", 0, item.file.size);
      xhr.send(item.file);
    });
    const next = () => {
      if (running) return;
      running = queue.shift() || null;
      if (!running) { finish(); return; }
      running.row.classList.add("busy");
      send(running).then(() => { running = null; next(); });
    };
    const cancel = (item) => {
      item.cancelled = true;
      const at = queue.indexOf(item);
      if (at >= 0) queue.splice(at, 1);
      if (item.xhr) item.xhr.abort();
      item.row.remove();
      rows.splice(rows.indexOf(item), 1);
      empty();
      finish();
    };
    const add = (file) => {
      const row = template.content.firstElementChild.cloneNode(true);
      const dot = file.name.lastIndexOf(".");
      const stem = dot > 0 ? file.name.slice(0, dot) : file.name;
      const title = row.querySelector(".title");
      title.textContent = stem;
      title.title = file.name;
      row.querySelector(".console").textContent = consoles[file.name.slice(dot + 1).toLowerCase()] || "";
      const item = { file, row, words: row.querySelector(".progress-words"), bar: row.querySelector(".row-bar i") };
      row.querySelector("[data-cancel]").addEventListener("click", () => cancel(item));
      const after = rows.length ? rows[rows.length - 1].row : card.querySelector(".game-row.head");
      after.after(row);
      rows.push(item);
      empty();
      if (file.size > limit) { settle(item, { step: "failed", why: "too-big" }); return; }
      queue.push(item);
      show(item, "Waiting");
    };
    const take = (files) => {
      Array.from(files).forEach(add);
      next();
    };
    adder.addEventListener("click", () => picker.click());
    picker.addEventListener("change", () => { take(picker.files); picker.value = ""; });
    card.addEventListener("dragover", (e) => { e.preventDefault(); card.classList.add("drop"); });
    card.addEventListener("dragleave", (e) => { if (!card.contains(e.relatedTarget)) card.classList.remove("drop"); });
    card.addEventListener("drop", (e) => { e.preventDefault(); card.classList.remove("drop"); take(e.dataTransfer.files); });
  }
})();
