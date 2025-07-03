(function () {
  function updateDisks() {
    var xhr = new XMLHttpRequest();
    xhr.open("GET", "/api/sysinfo", true);
    xhr.onreadystatechange = function () {
      if (xhr.readyState === 4) {
        if (xhr.status === 200) {
          try {
            var data = JSON.parse(xhr.responseText);
            var disksContainer = document.getElementById("disks");
            disksContainer.innerHTML = "";

            for (var i = 0; i < data.disks.length; i++) {
              var disk = data.disks[i];

              var diskDiv = document.createElement("div");

              diskDiv.innerHTML = '<label for="usage">' + disk.used_space_readable + "/" + disk.total_space_readable + '</label>' + '<progress style="width:100%; box-sizing:border-box;" class="disk_usage" max="' + disk.total_space + '" value="' + disk.used_space + '"></progress>';

              disksContainer.appendChild(diskDiv);
            }
          } catch (e) {
            if (window.console) console.error("JSON parse error:", e);
          }
        } else {
          if (window.console) console.error("HTTP error:", xhr.status);
        }
      }
    };
    xhr.onerror = function () {
      if (window.console)
        console.error("Network error occurred during /api/sysinfo fetch.");
    };
    xhr.send();
  }

  if (
    document.readyState === "complete" ||
    document.readyState === "interactive"
  ) {
    setTimeout(updateDisks, 0);
  } else if (document.addEventListener) {
    document.addEventListener("DOMContentLoaded", updateDisks, false);
  } else if (document.attachEvent) {
    document.attachEvent("onreadystatechange", function () {
      if (document.readyState === "complete") updateDisks();
    });
  }
})();
