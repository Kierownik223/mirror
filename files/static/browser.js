function openDialog() {
    var dialog = document.getElementById("browse_dialog");
    window.location.href = "#browse_dialog";
    if (dialog) dialog.style.display = "block";
}

function closeDialog() {
    var dialog = document.getElementById("browse_dialog");
    window.location.href = "#";
    if (dialog) dialog.style.display = "none";
}

function selectPath() {
    var iframe = document.getElementById("browser_iframe");
    var iframeDoc;

    try {
        iframeDoc = iframe.contentWindow
            ? iframe.contentWindow.document
            : iframe.document;
    } catch (e) {
        alert("Unable to access iframe content.");
        return;
    }

    var inputs = iframeDoc.getElementsByTagName("input");
    var pathValue = "";

    for (var i = 0; i < inputs.length; i++) {
        if (inputs[i].name === "path") {
            pathValue = inputs[i].value;
            break;
        }
    }

    var pathField = document.getElementById("path");
    if (pathField) {
        pathField.value = pathValue;
    }

    closeDialog();
}
