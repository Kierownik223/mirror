var video = document.getElementById("video");
var titleEl = document.getElementById("title");
var descriptionEl = document.getElementById("description");
var downloadEl = document.getElementById("download");
var breadcrumbsEl = document.getElementsByClassName("breadcrumbs")[0];
var previous = document.getElementById("previous");
var next = document.getElementById("next");
var autoplay = document.getElementById("autoplay");

var cookies = document.cookie.split(';');
for (var i = 0; i < cookies.length; i++) {
    var cookie = cookies[i].replace(/^\s+/, '');
    if (cookie.indexOf("audiovolume=") === 0) {
        video.volume = cookie.substring("audiovolume=".length);
    }
}

video.addEventListener("volumechange", function () {
    document.cookie = "audiovolume=" + video.volume + "; path=/"; 
});

function fetchJSON(url, callback) {
    var xhr;
    if (window.XMLHttpRequest) {
        xhr = new XMLHttpRequest();
    } else {
        try {
            xhr = new ActiveXObject("Microsoft.XMLHTTP");
        } catch (e) {
            alert("Failed to load resource: " + url);
            return;
        }
    }

    xhr.onreadystatechange = function () {
        if (xhr.readyState === 4) {
            if (xhr.status === 200) {
                try {
                    var data = JSON.parse(xhr.responseText);
                    callback(null, data);
                } catch (e) {
                    callback(e, null);
                }
            } else {
                callback(new Error("Request failed: " + xhr.status), null);
            }
        }
    };
    xhr.open("GET", url, true);
    xhr.send();
}

function updatePageMetadata(meta, newPath) {
    if (titleEl)
        titleEl.textContent =
            meta.title || decodeURIComponent(newPath.split("/").pop());
    if (descriptionEl) descriptionEl.innerHTML = meta.description || "";
    if (downloadEl) downloadEl.href = "/file" + newPath;
    if (breadcrumbsEl)
        breadcrumbsEl.outerHTML =
            createBreadcrumbs(decodeURIComponent(newPath)).outerHTML ||
            breadcrumbsEl;
    breadcrumbsEl = document.getElementsByClassName("breadcrumbs")[0];

    video.poster = "/poster" + newPath;

    if (window.navigator && navigator.mediaSession) {
        try {
            navigator.mediaSession.metadata = new MediaMetadata({
                title: meta.title,
                artwork: [
                    {
                        src: "/poster" + newPath,
                    },
                ],
            });
        } catch (e) {
            alert(e);
        }
    }

    if (meta.title) {
        meta.title += " - MARMAK Mirror";
    }

    document.title =
        meta.title ||
        decodeURIComponent(newPath.split("/").pop()) + " - MARMAK Mirror";

    if (window.history && history.pushState) {
        history.pushState(null, "", newPath);
    }
}

var pathname = window.location.pathname.split("/");
var currentFile = decodeURIComponent(pathname.pop());
var folderPath = decodeURIComponent(pathname.join("/"));
var fileNames = [];
var currentIndex = 0;

fetchJSON("/api/listing" + folderPath, function (err, files) {
    if (err) {
        console.error("Failed to fetch file list", err);
        return;
    }

    for (var i = 0; i < files.length; i++) {
        var lower = files[i].name.toLowerCase();

        if (lower.match(/\.(mp4|mkv|webm)$/)) {
            fileNames.push(files[i].name);
        }
    }

    video.addEventListener("ended", function () {
        if (autoplay.checked) {
            if (currentIndex + 1 >= fileNames.length - 1) {
                next.style.display = "none";
            } else {
                next.style.display = "inline";
            }
            if (currentIndex + 1 == 0 || fileNames.length == 1) {
                previous.style.display = "none";
            } else {
                previous.style.display = "inline";
            }
            if (currentIndex < fileNames.length - 1) {
                currentIndex++;
                loadVideo(currentIndex);
            }
        }
    });

    currentIndex = fileNames.indexOf(currentFile);
    if (currentIndex === -1) currentIndex = 0;

    if (currentIndex == fileNames.length - 1) {
        next.style.display = "none";
    } else {
        next.style.display = "inline";
    }
    if (currentIndex == 0) {
        previous.style.display = "none";
    } else {
        previous.style.display = "inline";
    }

    function loadVideo(index) {
        if (index < 0 || index >= fileNames.length) return;

        var targetFile = fileNames[index];
        var newPath = folderPath + "/" + encodeURIComponent(targetFile);

        video.src = "/file" + newPath;
        video.play();

        fetchJSON("/api" + newPath, function (err, meta) {
            if (!err && meta) {
                updatePageMetadata(meta, newPath);
            }
        });
    }

    previous.onclick = function () {
        if (currentIndex - 1 == fileNames.length) {
            next.style.display = "none";
        } else {
            next.style.display = "inline";
        }
        if (currentIndex - 1 == 0) {
            previous.style.display = "none";
        } else {
            previous.style.display = "inline";
        }
        if (currentIndex > 0) {
            currentIndex--;
            loadVideo(currentIndex);
        }
    };

    next.onclick = function () {
        if (currentIndex + 1 == fileNames.length - 1) {
            next.style.display = "none";
        } else {
            next.style.display = "inline";
        }
        if (currentIndex + 1 == 0) {
            previous.style.display = "none";
        } else {
            previous.style.display = "inline";
        }
        if (currentIndex < fileNames.length - 1) {
            currentIndex++;
            loadVideo(currentIndex);
        }
    };

    if (navigator.mediaSession) {
        try {
            navigator.mediaSession.setActionHandler(
                "previoustrack",
                function () {
                    if (currentIndex - 1 == fileNames.length) {
                        next.style.display = "none";
                    } else {
                        next.style.display = "inline";
                    }
                    if (currentIndex - 1 == 0) {
                        previous.style.display = "none";
                    } else {
                        previous.style.display = "inline";
                    }
                    if (currentIndex > 0) {
                        currentIndex--;
                        loadVideo(currentIndex);
                    }
                }
            );

            navigator.mediaSession.setActionHandler("nexttrack", function () {
                if (currentIndex + 1 == fileNames.length - 1) {
                    next.style.display = "none";
                } else {
                    next.style.display = "inline";
                }
                if (currentIndex + 1 == 0) {
                    previous.style.display = "none";
                } else {
                    previous.style.display = "inline";
                }
                if (currentIndex < fileNames.length - 1) {
                    currentIndex++;
                    loadVideo(currentIndex);
                }
            });
        } catch (e) {
            alert(e);
        }
    }
});
