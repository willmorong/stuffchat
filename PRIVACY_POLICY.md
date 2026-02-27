# Stuffchat Privacy Policy

**Effective Date:** February 27, 2026
**Last Updated:** February 27, 2026

This Privacy Policy describes how William Morong ("Developer," "we," "us," or "our") handles information in connection with the Stuffchat software ("Software," "Service," or "Application"). By registering for, accessing, or using Stuffchat, you ("User," "you," or "your") acknowledge that you have read, understood, and agree to be bound by this Privacy Policy. If you do not agree with this Privacy Policy, do not use the Service.

---

## 1. Nature of the Service

Stuffchat is a **self-hosted**, open-source chat application. Each instance of Stuffchat is independently deployed and operated by an instance administrator ("Instance Operator"). The Developer provides the software only and **does not operate, control, or have access to any data on any self-hosted instance** unless the Developer is also the Instance Operator.

> **If you are using an instance operated by someone other than the Developer, that Instance Operator is independently responsible for data handling on their instance. This Privacy Policy does not apply to any instance not operated by the Developer. You accept full responsibility for sharing any information with any instance.**

---

## 2. Information We Collect

### 2.1 Account Information
When you register for an account, we collect:
- **Username** — your chosen display name.
- **Email address** — used for account identification.
- **Password** — stored exclusively as a cryptographic hash using the Argon2 algorithm. We **never** store or have access to your plaintext password.
- **Account metadata** — timestamps of account creation and updates.

### 2.2 Profile Information
You may optionally provide:
- **Profile picture / avatar** — an image file you upload.

### 2.3 Communications Data
When you use the Service, we store:
- **Messages** — text content you send in channels, including edits and the timestamps of edits. Deleted messages are soft-deleted (marked as deleted) and may be retained in the database.
- **File attachments** — files you upload and attach to messages, including original filename, MIME type, and file size.
- **Reactions** — emoji reactions you add to messages.
- **Replies** — associations between reply messages and the messages they reference.

### 2.4 Channel and Membership Data
- **Channel information** — names, types (text, voice, public, private), and creator information for channels you create or participate in.
- **Channel membership** — records of which channels you are a member of and your permissions within them.

### 2.5 Presence and Status Data
- **Online presence** — your last heartbeat timestamp and status setting (online, away, do not disturb, invisible, or offline).

### 2.6 Voice and Video Data
- **WebRTC signaling data** — session descriptions and ICE candidates exchanged to establish peer-to-peer voice calls and screen sharing connections. This data is transient and used only to negotiate connections between participants.
- **Voice and video streams** — audio and video data in voice calls and screen sharing sessions are transmitted **peer-to-peer** between participants using WebRTC. This media data **does not pass through or get stored on the server**.

### 2.7 Synced Playlist Data
- **Playlist and media metadata** — information related to synced listening sessions, including media URLs and playback state, which may be automatically downloaded and cached.

### 2.8 Authentication Tokens
- **Refresh tokens** — stored as cryptographic hashes; used to maintain your login session. These tokens have a 30-day expiration and are revoked upon rotation. Expired and revoked tokens are periodically cleaned up.
- **Access tokens (JWT)** — short-lived (15-minute) tokens used for API authentication. These are not stored server-side.

### 2.9 Invite Codes
- **Invite codes** — if the instance is configured as invite-only, we store invite codes, who created them, and which user (if any) redeemed them.

### 2.10 Search Index Data
- **Full-text search index** — message content is indexed using SQLite FTS5 to enable message search functionality. This index mirrors stored message content.

### 2.11 Custom Emoji Data
- **Custom emojis** — emoji names, associated image files, and the user who uploaded them.

---

## 3. How We Use Your Information

We use the information collected solely for the following purposes:
- **Providing the Service** — delivering chat, file sharing, voice/video calling, screen sharing, and related functionality.
- **Authentication and security** — verifying your identity, managing sessions, and protecting accounts.
- **Search functionality** — enabling you to search through message history.
- **Presence and status** — displaying your online status to other users on the instance.
- **Instance administration** — managing users, channels, roles, and permissions.

We do **not** use your information for:
- Advertising or ad targeting.
- Selling or renting to third parties.
- Profiling, analytics, or behavioral tracking.
- Training machine learning or AI models.

---

## 4. Data Storage and Security

### 4.1 Storage
All data is stored locally on the server hosting the Stuffchat instance, using:
- **SQLite database** — for structured data (users, messages, channels, etc.).
- **File system** — for uploaded files and attachments in the configured uploads directory.

No data is transmitted to external cloud services, third-party analytics platforms, or remote servers operated by the Developer (unless the Developer is the Instance Operator).

### 4.2 Security Measures
We implement the following security measures:
- **Password hashing** — all passwords are hashed using Argon2, a memory-hard hashing algorithm resistant to brute-force and GPU-based attacks.
- **Token security** — refresh tokens are stored as hashed values; access tokens are short-lived JWTs.
- **HTTPS** — the recommended deployment configuration uses TLS encryption (via Caddy or equivalent reverse proxy) for all data in transit.
- **CORS restrictions** — the server enforces Cross-Origin Resource Sharing restrictions to prevent unauthorized cross-origin requests.
- **Invite-only registration** — the instance can be configured to require invite codes for new registrations.
- **Peer-to-peer media** — voice, video, and screen sharing data travels directly between users via WebRTC and is not stored on the server.

### 4.3 Security Limitations
Despite these measures, **no method of electronic storage or transmission is 100% secure**. We cannot guarantee absolute security of your data. You use the Service at your own risk.

---

## 5. Data Sharing and Disclosure

We do **not** sell, rent, trade, or otherwise share your personal information with third parties, except:
- **As required by law** — we may disclose information if required to do so by law, regulation, legal process, or governmental request.
- **To protect rights and safety** — we may disclose information if we believe in good faith that disclosure is necessary to protect our rights, your safety, the safety of others, or to investigate fraud or security issues.
- **With your consent** — we may share information with your explicit consent.
- **Visible to other users** — your username, avatar, messages, reactions, presence status, and other content you post are visible to other authorized users of the instance as part of normal Service functionality.

---

## 6. Data Retention

- **Account data** — retained for as long as your account exists.
- **Messages** — retained indefinitely unless deleted by you or an administrator. Deleted messages are soft-deleted and may remain in the database.
- **Files and attachments** — retained on the server until manually removed by an administrator.
- **Refresh tokens** — expired and revoked tokens are periodically purged.
- **Presence data** — continuously updated and overwritten; no historical presence data is retained.

---

## 7. Your Rights and Choices

Depending on your jurisdiction, you may have certain rights regarding your personal data, including:
- **Access** — you may request information about the data we hold about you.
- **Correction** — you may update your username, email, password, and avatar through the Service's settings.
- **Deletion** — you may request deletion of your account and associated data by contacting the Instance Operator. Note that some data (e.g., messages in channels) may be retained in anonymized or aggregated form.
- **Data portability** — since all data is stored in a standard SQLite database, the Instance Operator can export your data upon request.

To exercise these rights, contact the Instance Operator of the instance you use. If the Developer is the Instance Operator, contact information is provided in Section 11.

---

## 8. Third-Party Services and WebRTC

### 8.1 WebRTC and STUN/TURN Servers
Voice calls and screen sharing use WebRTC, which may communicate with public STUN servers to establish peer-to-peer connections. STUN servers may receive your IP address as part of the ICE candidate exchange process. This is an inherent aspect of WebRTC technology and is necessary for establishing direct connections between users.

### 8.2 No Other Third-Party Integrations
The Software does not integrate with or transmit data to any third-party analytics, advertising, or tracking services.

---

## 9. Disclaimer of Warranties and Limitation of Liability

THE SOFTWARE IS PROVIDED "AS IS" AND "AS AVAILABLE," WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE, AND NON-INFRINGEMENT.

IN NO EVENT SHALL THE DEVELOPER BE LIABLE FOR ANY INDIRECT, INCIDENTAL, SPECIAL, CONSEQUENTIAL, OR PUNITIVE DAMAGES, INCLUDING BUT NOT LIMITED TO LOSS OF DATA, LOSS OF PROFITS, OR BUSINESS INTERRUPTION, ARISING OUT OF OR IN CONNECTION WITH YOUR USE OF OR INABILITY TO USE THE SERVICE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGES.

THE DEVELOPER IS NOT RESPONSIBLE FOR:
- The conduct, content, or communications of any users on any instance.
- Data breaches, data loss, or unauthorized access caused by the Instance Operator's configuration, infrastructure, or security practices.
- Content uploaded, shared, or transmitted by users through the Service.
- The actions or omissions of any Instance Operator other than the Developer.

YOUR USE OF THE SERVICE IS AT YOUR SOLE RISK. YOU ARE SOLELY RESPONSIBLE FOR THE CONTENT YOU SHARE AND THE INFORMATION YOU PROVIDE.

---

## 10. Changes to This Privacy Policy

We reserve the right to modify this Privacy Policy at any time. Changes will be effective upon posting the updated Privacy Policy with a revised "Last Updated" date. Your continued use of the Service after any changes constitutes your acceptance of the revised Privacy Policy. We encourage you to review this Privacy Policy periodically.

---

## 11. Contact Information

If you have questions, concerns, or requests regarding this Privacy Policy, please write on the issues page on GitHub.
---

## 12. Governing Law

This Privacy Policy shall be governed by and construed in accordance with the laws of the jurisdiction in which the Developer resides, without regard to its conflict of law provisions.

---

## 13. Severability

If any provision of this Privacy Policy is found to be unenforceable or invalid, that provision shall be limited or eliminated to the minimum extent necessary so that this Privacy Policy shall otherwise remain in full force and effect.

---

*This Privacy Policy applies to the Stuffchat software developed by William Morong and distributed under the MIT License.*
