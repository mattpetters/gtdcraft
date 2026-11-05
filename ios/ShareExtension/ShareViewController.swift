// GTDCraft Share Extension — receive shared text/URLs from other apps, create an inbox item
// with a Reference to the shared content. Posts to the sync server (POST /v1/inbox).
// Server URL and token are stored in the extension's own UserDefaults (entered on first use).
// ponytail: no App Groups on a free Apple account, so the extension can't read the main app's data.

import UIKit
import UniformTypeIdentifiers

class ShareViewController: UIViewController {
    private let serverField = UITextField()
    private let tokenField = UITextField()
    private let nameField = UITextField()
    private let statusLabel = UILabel()
    private let sendButton = UIButton(type: .system)
    private let cancelButton = UIButton(type: .system)

    private var sharedText = ""
    private var sharedURL = ""
    private var sharedTitle = ""

    private let defaults = UserDefaults.standard
    private let serverKey = "gtdcraft_server_url"
    private let tokenKey = "gtdcraft_server_token"

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .systemBackground
        extractSharedContent()
        setupUI()
    }

    private func extractSharedContent() {
        guard let items = extensionContext?.inputItems as? [NSExtensionItem] else { return }
        let group = DispatchGroup()
        for item in items {
            guard let attachments = item.attachments else { continue }
            for provider in attachments {
                if provider.hasItemConformingToTypeIdentifier(UTType.url.identifier) {
                    group.enter()
                    provider.loadItem(forTypeIdentifier: UTType.url.identifier) { [weak self] data, _ in
                        if let url = data as? URL {
                            self?.sharedURL = url.absoluteString
                            self?.sharedTitle = url.host ?? url.absoluteString
                        }
                        group.leave()
                    }
                } else if provider.hasItemConformingToTypeIdentifier(UTType.plainText.identifier) {
                    group.enter()
                    provider.loadItem(forTypeIdentifier: UTType.plainText.identifier) { [weak self] data, _ in
                        if let text = data as? String {
                            self?.sharedText = text
                        }
                        group.leave()
                    }
                }
            }
            // Also grab the attributed title if available
            if let title = item.attributedContentText?.string, !title.isEmpty {
                sharedTitle = title
            }
        }
        group.notify(queue: .main) { [weak self] in
            self?.populateFields()
        }
    }

    private func populateFields() {
        if sharedURL.isEmpty && !sharedText.isEmpty {
            // Check if the shared text looks like a URL
            if let url = URL(string: sharedText), url.scheme?.hasPrefix("http") == true {
                sharedURL = sharedText
                sharedTitle = url.host ?? sharedText
                nameField.text = sharedTitle
            } else {
                nameField.text = String(sharedText.prefix(80))
            }
        } else if !sharedURL.isEmpty {
            nameField.text = sharedTitle.isEmpty ? sharedURL : sharedTitle
        }
    }

    private func setupUI() {
        let stack = UIStackView()
        stack.axis = .vertical
        stack.spacing = 12
        stack.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.topAnchor.constraint(equalTo: view.safeAreaLayoutGuide.topAnchor, constant: 20),
            stack.leadingAnchor.constraint(equalTo: view.leadingAnchor, constant: 20),
            stack.trailingAnchor.constraint(equalTo: view.trailingAnchor, constant: -20),
        ])

        let title = UILabel()
        title.text = "Add to GTDCraft Inbox"
        title.font = .preferredFont(forTextStyle: .headline)
        stack.addArrangedSubview(title)

        configureField(nameField, placeholder: "Task name", text: "")
        stack.addArrangedSubview(nameField)

        // the app's own connection (shared App Group) unless this extension was given one
        let shared = Self.appGroupDefaults()
        let stored = defaults.string(forKey: serverKey) ?? shared?.string(forKey: "sync_url") ?? ""
        let storedToken = defaults.string(forKey: tokenKey) ?? shared?.string(forKey: "sync_token") ?? ""

        if stored.isEmpty || storedToken.isEmpty {
            let hint = UILabel()
            hint.text = "Enter your sync server details (Settings > Sync on the Mac shows these):"
            hint.font = .preferredFont(forTextStyle: .caption1)
            hint.textColor = .secondaryLabel
            hint.numberOfLines = 0
            stack.addArrangedSubview(hint)
        }

        configureField(serverField, placeholder: "Server URL (e.g. http://mac.local:8765)", text: stored)
        stack.addArrangedSubview(serverField)

        configureField(tokenField, placeholder: "Token", text: storedToken)
        tokenField.isSecureTextEntry = true
        stack.addArrangedSubview(tokenField)

        if !sharedURL.isEmpty {
            let refLabel = UILabel()
            refLabel.text = "Reference: \(sharedURL)"
            refLabel.font = .preferredFont(forTextStyle: .caption1)
            refLabel.textColor = .secondaryLabel
            refLabel.numberOfLines = 2
            stack.addArrangedSubview(refLabel)
        }

        statusLabel.font = .preferredFont(forTextStyle: .caption1)
        statusLabel.textColor = .systemRed
        statusLabel.numberOfLines = 0
        stack.addArrangedSubview(statusLabel)

        let buttons = UIStackView()
        buttons.axis = .horizontal
        buttons.spacing = 16
        buttons.distribution = .fillEqually

        cancelButton.setTitle("Cancel", for: .normal)
        cancelButton.addTarget(self, action: #selector(cancelTapped), for: .touchUpInside)
        buttons.addArrangedSubview(cancelButton)

        sendButton.setTitle("Add to Inbox", for: .normal)
        sendButton.setTitleColor(.white, for: .normal)
        sendButton.backgroundColor = .systemBlue
        sendButton.layer.cornerRadius = 8
        sendButton.addTarget(self, action: #selector(sendTapped), for: .touchUpInside)
        buttons.addArrangedSubview(sendButton)

        stack.addArrangedSubview(buttons)
    }

    private func configureField(_ field: UITextField, placeholder: String, text: String) {
        field.placeholder = placeholder
        field.text = text
        field.borderStyle = .roundedRect
        field.autocorrectionType = .no
        field.autocapitalizationType = .none
    }

    @objc private func cancelTapped() {
        extensionContext?.completeRequest(returningItems: nil)
    }

    @objc private func sendTapped() {
        let server = (serverField.text ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        let token = (tokenField.text ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        let name = (nameField.text ?? "").trimmingCharacters(in: .whitespacesAndNewlines)

        guard !server.isEmpty, !token.isEmpty else {
            statusLabel.text = "Server URL and token are required."
            return
        }
        guard !name.isEmpty || !sharedURL.isEmpty else {
            statusLabel.text = "A task name or shared URL is required."
            return
        }

        // Save server details for next time
        defaults.set(server, forKey: serverKey)
        defaults.set(token, forKey: tokenKey)

        sendButton.isEnabled = false
        statusLabel.text = "Sending..."
        statusLabel.textColor = .secondaryLabel

        postToInbox(server: server, token: token, name: name)
    }

    /// The App Group shared with the app (GTDAppGroup in Info.plist, from GTD_APP_GROUP in ios/Config).
    static func appGroupDefaults() -> UserDefaults? {
        guard let group = Bundle.main.object(forInfoDictionaryKey: "GTDAppGroup") as? String, !group.isEmpty else { return nil }
        return UserDefaults(suiteName: group)
    }

    private func postToInbox(server: String, token: String, name: String) {
        let db = Self.appGroupDefaults()?.string(forKey: "sync_db") ?? ""
        let endpoint = "\(server.trimmingCharacters(in: .init(charactersIn: "/")))/v1/inbox?db=\(db)"
        guard let url = URL(string: endpoint) else {
            statusLabel.text = "Invalid server URL."
            statusLabel.textColor = .systemRed
            sendButton.isEnabled = true
            return
        }

        var body: [String: String] = ["name": name]
        if !sharedURL.isEmpty {
            body["ref_url"] = sharedURL
            body["ref_title"] = sharedTitle
        }
        if !sharedText.isEmpty && sharedText != sharedURL {
            body["note"] = sharedText
        }

        var req = URLRequest(url: url)
        req.httpMethod = "POST"
        req.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        req.setValue("application/json", forHTTPHeaderField: "Content-Type")
        req.timeoutInterval = 10
        req.httpBody = try? JSONSerialization.data(withJSONObject: body)

        URLSession.shared.dataTask(with: req) { [weak self] data, response, error in
            DispatchQueue.main.async {
                if let error = error {
                    self?.statusLabel.text = "Error: \(error.localizedDescription)"
                    self?.statusLabel.textColor = .systemRed
                    self?.sendButton.isEnabled = true
                    return
                }
                let code = (response as? HTTPURLResponse)?.statusCode ?? 0
                if code == 200 {
                    self?.extensionContext?.completeRequest(returningItems: nil)
                } else if code == 401 {
                    self?.statusLabel.text = "Wrong token."
                    self?.statusLabel.textColor = .systemRed
                    self?.sendButton.isEnabled = true
                } else {
                    self?.statusLabel.text = "Server error (\(code))."
                    self?.statusLabel.textColor = .systemRed
                    self?.sendButton.isEnabled = true
                }
            }
        }.resume()
    }
}
