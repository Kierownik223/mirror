(function () {
  function logError(msg, err) {
    if (window.console && typeof console.error === "function") {
      console.error(msg, err || "");
    }
  }

  function updateInfo() {
    var xhr;
    if (window.XMLHttpRequest) {
      xhr = new XMLHttpRequest();
    } else {
      xhr = new ActiveXObject("Microsoft.XMLHTTP");
    }

    xhr.open("GET", "/api/sysinfo", true);

    xhr.onreadystatechange = function () {
      if (xhr.readyState === 4) {
        if (xhr.status === 200 || xhr.status === 0) {
          try {
            var data = JSON.parse(xhr.responseText);

            var usedMem = document.getElementById("used_mem_readable");
            var totalMem = document.getElementById("total_mem_readable");
            var memUsage = document.getElementById("mem_usage");
            var disksContainer = document.getElementById("disks");

            if (usedMem) {
              if ("textContent" in usedMem) {
                usedMem.textContent = data.used_mem_readable;
              } else {
                usedMem.innerText = data.used_mem_readable;
              }
            }

            if (totalMem) {
              if ("textContent" in totalMem) {
                totalMem.textContent = data.total_mem_readable;
              } else {
                totalMem.innerText = data.total_mem_readable;
              }
            }

            if (memUsage) {
              memUsage.max = data.total_mem;
              memUsage.value = data.used_mem;
            }

            if (disksContainer) {
              disksContainer.innerHTML = "";

              for (var i = 0; i < data.disks.length; i++) {
                var disk = data.disks[i];
                var diskDiv = document.createElement("div");

                diskDiv.innerHTML = '<label for="usage">' + disk.used_space_readable + "/" + disk.total_space_readable + '</label>' + '<progress style="width:100%; box-sizing:border-box;" class="disk_usage" max="' + disk.total_space + '" value="' + disk.used_space + '"></progress>';

                disksContainer.appendChild(diskDiv);
              }
            }
          } catch (e) {
            logError("JSON parse error:", e);
          }
        } else {
          logError("Failed to fetch system info. Status:", xhr.status);
        }
      }
    };

    try {
      xhr.send();
    } catch (e) {
      logError("Request send error:", e);
    }
  }

  function init() {
    updateInfo();
    setInterval(updateInfo, 2500);
  }

  function domReady(fn) {
    if (document.readyState === "complete" || document.readyState === "interactive") {
      setTimeout(fn, 0);
    } else if (document.addEventListener) {
      document.addEventListener("DOMContentLoaded", fn, false);
    } else if (document.attachEvent) {
      document.attachEvent("onreadystatechange", function () {
        if (document.readyState === "complete") fn();
      });
    } else {
      window.onload = fn;
    }
  }

  domReady(init);
})();