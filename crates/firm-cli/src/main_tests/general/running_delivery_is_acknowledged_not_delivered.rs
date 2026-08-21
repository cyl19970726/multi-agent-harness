use super::*;

#[test]
fn running_delivery_is_acknowledged_not_delivered() {
    assert_eq!(
        message_status_for_delivery(&ProviderExecutionStatus::Running),
        RegistryDeliveryStatus::Acknowledged
    );
    assert_eq!(
        message_status_for_delivery(&ProviderExecutionStatus::Succeeded),
        RegistryDeliveryStatus::Delivered
    );
    assert_eq!(
        message_status_for_delivery(&ProviderExecutionStatus::Failed),
        RegistryDeliveryStatus::Failed
    );
}
