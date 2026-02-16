function sendRequest(options) {
    var xhr;

    if (window.XMLHttpRequest) {
        xhr = new XMLHttpRequest();
    } else {
        try {
            xhr = new ActiveXObject("Microsoft.XMLHTTP");
        } catch (e) {
            if (options.onInitError) options.onInitError(e);
            return null;
        }
    }

    try {
        xhr.open(options.method, options.url, true);
        if (options.contentType) xhr.setRequestHeader("Content-Type", options.contentType);
    } catch (e) {
        if (options.onOpenError) options.onOpenError(e);
        return null;
    }

    xhr.onreadystatechange = function () {
        if (options.onReady) {
            options.onReady(xhr);
        }
    };

    try {
        if (options.body) {
            xhr.send(options.body);
        } else {
            xhr.send();
        }
    } catch (e) {
        if (options.onSendError) options.onSendError(e);
    }

    return xhr;
}
