pub(super) fn sanitize_message(message: String) -> String {
    message.replace(".", "-")
}
